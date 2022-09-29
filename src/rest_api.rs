use std::convert::identity;
use std::net::{SocketAddr, IpAddr};
use std::cmp::min;
use std::ops::Deref;
use std::time::Duration;
use borsh::{BorshSerialize, BorshDeserialize};
use tokio;
use serde;
use warp::hyper::StatusCode;
use warp::{self, http, Filter};
use reqwest;
use pchain_types::Base64URL;
use crate::msg_types::{Block, BlockHash, BlockHeight};
use crate::block_tree::{BlockTreeSnapshotFactory, BlockTreeSnapshot, ChildrenNotYetCommittedError};
use crate::config::BlockTreeApiConfig;
use crate::stored_types::Key;

pub(crate) struct Server(tokio::runtime::Runtime);

impl Server {
    pub(crate) fn start(block_tree_snapshot_factory: BlockTreeSnapshotFactory, config: BlockTreeApiConfig) -> Server {
        // GET /blocks
        let block_tree_snapshot_factory1 = block_tree_snapshot_factory.clone();
        let get_blocks = warp::path("blocks")
            .and(warp::query::<GetBlocksParams>())
            .then(move |params| Self::handle_get_blocks(block_tree_snapshot_factory1.clone(), params));

        // GET /storage
        let get_storage = warp::path("storage")
            .and(warp::query::<GetStorageParams>())
            .then(move |params| Self::handle_get_storage(block_tree_snapshot_factory.clone(), params));

        let server = warp::serve(get_blocks.or(get_storage));
        let listening_socket_addr = SocketAddr::new(config.listening_addr, config.listening_port); 
        let task = server.bind(listening_socket_addr);

        let runtime = tokio::runtime::Runtime::new()
            .expect("Programming or Configuration error: fail to create Tokio runtime.");
        let _ = runtime.spawn(task);

        Server(runtime)
    }

    async fn handle_get_blocks(block_tree_snapshot_factory: BlockTreeSnapshotFactory, params: GetBlocksParams) -> impl warp::Reply {
        let block_tree_snapshot = block_tree_snapshot_factory.snapshot();

        // Interpret query parameters
        let hash = match (params.hash, params.height) {
            (Some(hash), None) => match Base64URL::decode(&hash) {
                Err(_) => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
                Ok(hash) => match hash.try_into() {
                    Ok(hash) => hash,
                    Err(_) => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
                }
            },
            (None, Some(height)) => match block_tree_snapshot.get_committed_block_hash(height as u64) {
                None => return http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()), 
                Some(hash) => hash,
            },
            _ => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
        };

        let anchor = params.anchor.map_or(GetBlocksAnchor::Head, identity);
        let limit = params.limit.map_or(1, identity);
        let speculate = params.speculate.map_or(false, identity);

        match anchor {
            GetBlocksAnchor::Head => match get_blocks_up_to_head(&block_tree_snapshot, &hash, limit, speculate) {
                Some(blocks) => http::Response::builder().status(StatusCode::OK).body(blocks.try_to_vec().unwrap()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            },
            GetBlocksAnchor::Tail => match get_blocks_from_tail(&block_tree_snapshot, &hash, limit, speculate) {
                Some(blocks) => http::Response::builder().status(StatusCode::OK).body(blocks.try_to_vec().unwrap()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            }, 
        }
    }

    async fn handle_get_storage(block_tree_snapshot_factory: BlockTreeSnapshotFactory, params: GetStorageParams) -> impl warp::Reply {
        let block_tree_snapshot = block_tree_snapshot_factory.snapshot();
        
        let key = match Base64URL::decode(&params.key) {
            Err(_) => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
            Ok(key) => key,
        };

        match block_tree_snapshot.get_from_storage(&key) {
            Some(value) => http::Response::builder().status(StatusCode::OK).body(value.try_to_vec().unwrap()),
            None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GetBlocksParams {
    hash: Option<String>,
    height: Option<usize>,
    anchor: Option<GetBlocksAnchor>,
    limit: Option<usize>,
    speculate: Option<bool>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GetStorageParams {
    key: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum GetBlocksAnchor {
    Head,
    Tail,
}

fn get_blocks_from_tail(block_tree: &BlockTreeSnapshot, tail_block_hash: &BlockHash, limit: usize, speculate: bool) -> Option<Vec<Block>> {
    let mut res = Vec::with_capacity(limit);

    // 1. Get tail block.
    let tail_block = block_tree.get_block_by_hash(tail_block_hash)?;
    let mut cursor = tail_block.hash;
    res.push(tail_block);

    // 2. Walk through tail block's descendants until limit is satisfied or we hit uncommitted blocks.
    while res.len() < limit {
        let child = match block_tree.get_committed_block_child(&cursor) {
            Ok(block) => block,
            Err(ChildrenNotYetCommittedError) => break,
        };
        cursor = child.hash;
        res.push(child);
    }

    // 3. If limit is not yet satisfied and we are allowed to speculate, get speculative blocks.
    if res.len() < limit && speculate {
        if let Some(block) = block_tree.get_top_block() {
            // We reverse (.rev) uncommitted blocks so that blocks with lower heights appear first.
            let uncommitted_blocks: Vec<Block> = get_chain_between_speculative_block_and_highest_committed_block(&block_tree, &block.hash).into_iter().rev().collect();
            res.extend_from_slice(&uncommitted_blocks[..min(limit - res.len(), uncommitted_blocks.len())]);
        }
    }

    Some(res)
}

fn get_blocks_up_to_head(block_tree: &BlockTreeSnapshot, head_block_hash: &BlockHash, limit: usize, speculate: bool) -> Option<Vec<Block>> {
    let mut res = Vec::with_capacity(limit);

    // 1. Get head block.
    let head_block = block_tree.get_block_by_hash(head_block_hash)?;
    let head_block_parent_hash = head_block.justify.block_hash;
    let head_block_height = head_block.height;
    res.push(head_block);
    if limit == 1 {
        // 1.1. If only one block is requested, return immediately.
        return Some(res)
    }

    // 2. Check whether head block is speculative.
    let highest_committed_block = block_tree.get_highest_committed_block();
    if highest_committed_block.is_none() || head_block_height > highest_committed_block.as_ref().unwrap().height {
        // Start block IS speculative. 
        if !speculate {
            return None
        }

        // 2.1. Get speculative ancestors of head block.
        let speculative_ancestors: Vec<Block> = get_chain_between_speculative_block_and_highest_committed_block(block_tree, &head_block_hash).into_iter().rev().collect();
        res.extend_from_slice(&speculative_ancestors[..min(limit - res.len(), speculative_ancestors.len())]);

        // 2.2. Get non-speculative ancestors of head block, up to the number necessary to satisfy limit.
        if let Some(block) = highest_committed_block {
            let mut cursor = block;
            while limit - res.len() > 0 {
                let next_hash = cursor.justify.block_hash;
                let cursor_height = cursor.height;
                res.push(cursor);
                if cursor_height == 0 {
                    break
                }

                cursor = block_tree.get_block_by_hash(&next_hash).unwrap();
            }
        } 
    } else {
        // Start block IS NOT speculative.

        // 2.1. Get ancestors of head block, up to the number necessary to satisfy limit.
        let mut cursor = block_tree.get_block_by_hash(&head_block_parent_hash).unwrap();
        while limit - res.len() > 0 {
            let next_hash = cursor.justify.block_hash;
            let cursor_height = cursor.height;
            res.push(cursor);
            if cursor_height == 0 {
                break
            }

            cursor = block_tree.get_block_by_hash(&next_hash).unwrap();
        }

    }

    Some(res)
}

// Unintuitive behaviors: 
// 1. The returned chain does not include the highest_committed_block (it stops short on its child).
// 2. The returned chain is ordered from GREATER block height to SMALLER block height.
// 3. If highest committed block is None, returns an empty vector.
//
// # Panic
// if head_block_hash is not in the BlockTree.
fn get_chain_between_speculative_block_and_highest_committed_block(block_tree: &BlockTreeSnapshot, head_block_hash: &BlockHash) -> Vec<Block> {
    let mut res = Vec::new();

    let head_block = block_tree.get_block_by_hash(head_block_hash).unwrap();
    if let Some(highest_committed_block) = block_tree.get_highest_committed_block() {
        let mut cursor = head_block.justify.block_hash;
        res.push(head_block);

        while cursor != highest_committed_block.hash {
            let block = block_tree.get_block_by_hash(&cursor).unwrap();
            cursor = block.justify.block_hash;
            res.push(block);
        }
    };

    res
}

pub(crate) struct SyncModeClient(reqwest::blocking::Client);

impl SyncModeClient {
    pub fn new(request_timeout: Duration) -> SyncModeClient {
        let reqwest_client = reqwest::blocking::Client::builder().timeout(request_timeout).build()
            .expect("Programming or Configuration error: failed to open a HTTP client for SyncModeClient.");
        SyncModeClient(reqwest_client)
    }

    pub fn get_blocks_from_tail(&self, tail_block_height: BlockHeight, limit: usize, participant_ip_addr: &IpAddr) -> Result<Vec<Block>, GetBlocksFromTailError> {
        let url = format!(
            "http://{}/blocks?height={}&anchor=tail&limit={}&speculate=true",
            participant_ip_addr,
            tail_block_height,
            limit,
        );
        let resp = self.0.get(url).send().map_err(Self::convert_request_error)?;
        let resp_bytes = resp.bytes().map_err(|_| GetBlocksFromTailError::BadResponse)?; 
        let blocks = Vec::<Block>::deserialize(&mut &resp_bytes[..]).map_err(|_| GetBlocksFromTailError::BadResponse)?;

        Ok(blocks)
    }

    fn convert_request_error(e: reqwest::Error) -> GetBlocksFromTailError {
        if e.is_timeout() {
            GetBlocksFromTailError::RequestTimedOut
        } else if e.is_connect() {
            GetBlocksFromTailError::FailToConnectToServer
        } else if let Some(status) = e.status() {
            if status == StatusCode::NOT_FOUND {
                GetBlocksFromTailError::TailBlockNotFound
            } else {
                GetBlocksFromTailError::BadResponse
            }
        } else {
            GetBlocksFromTailError::BadResponse
        }
    }
}

pub(crate) enum GetBlocksFromTailError {
    FailToConnectToServer,
    RequestTimedOut,
    TailBlockNotFound,
    BadResponse,
}

