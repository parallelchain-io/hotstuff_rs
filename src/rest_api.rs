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
use crate::msg_types::{Block, BlockHash};
use crate::block_tree::{BlockTreeSnapshotFactory, BlockTreeSnapshot, ChildrenNotYetCommittedError};
use crate::config::BlockTreeApiConfig;

pub(crate) struct Server(tokio::runtime::Runtime);

impl Server {
    pub(crate) fn start(block_tree_snapshot_factory: BlockTreeSnapshotFactory, config: BlockTreeApiConfig) -> Server {
        // Endpoints and their behavior are documented in block_tree/rest_api.md

        // GET /blocks
        let get_blocks = warp::path("blocks")
            .and(warp::query::<GetBlocksParams>())
            .then(move |blocks_params| Self::handle_get_blocks(block_tree_snapshot_factory.clone(), blocks_params));

        let server = warp::serve(get_blocks);
        let listening_socket_addr = SocketAddr::new(config.listening_addr, config.listening_port); 
        let task = server.bind(listening_socket_addr);

        let runtime = tokio::runtime::Runtime::new()
            .expect("Programming or Configuration error: fail to create Tokio runtime.");
        let _ = runtime.spawn(task);

        Server(runtime)
    }

    async fn handle_get_blocks<'a>(block_tree_snapshot_factory: BlockTreeSnapshotFactory, params: GetBlocksParams) -> impl warp::Reply {
        // Interpret query parameters
        let hash = match Base64URL::decode(&params.hash) {
            Err(_) => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
            Ok(hash) => match hash.try_into() {
                Ok(hash) => hash,
                Err(_) => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
            }
        };
        let anchor = match params.anchor {
            None => GetBlocksAnchor::Head,
            Some(anchor) => anchor,
        };
        let limit = match params.limit {
            None => 1,
            Some(limit) => limit,
        };
        let speculate = match params.speculate {
            None => false,
            Some(speculate) => speculate,
        };

        let block_tree_snapshot = block_tree_snapshot_factory.snapshot();

        match anchor {
            GetBlocksAnchor::Head => match get_blocks_up_to_head(&block_tree_snapshot, &hash, limit, speculate) {
                Some(blocks) => http::Response::builder().status(StatusCode::OK).body(blocks.try_to_vec().unwrap()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            },
            GetBlocksAnchor::Tail => match get_blocks_from_tail(&block_tree_snapshot, &hash, limit, speculate) {
                Some(blocks) => http::Response::builder().status(StatusCode::OK).body(blocks.try_to_vec().unwrap()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            }, 
            _ => unreachable!()
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GetBlocksParams {
    hash: String,
    anchor: Option<GetBlocksAnchor>,
    limit: Option<usize>,
    speculate: Option<bool>,
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
        let top_block = block_tree.get_top_block();
        // 3.1. Reverse uncommitted blocks so that blocks with lower heights appear first.
        let uncommitted_blocks: Vec<Block> = get_chain_between_speculative_block_and_highest_committed_block(&block_tree, &top_block.hash).into_iter().rev().collect();
        res.extend_from_slice(&uncommitted_blocks[..min(limit - res.len(), uncommitted_blocks.len())]);
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
    if head_block_height > highest_committed_block.height {
        // Start block IS speculative. 
        if !speculate {
            return None
        }

        // 2.1. Get speculative ancestors of head block.
        let speculative_ancestors: Vec<Block> = get_chain_between_speculative_block_and_highest_committed_block(block_tree, &head_block_hash).into_iter().rev().collect();
        res.extend_from_slice(&speculative_ancestors[..min(limit - res.len(), speculative_ancestors.len())]);

        // 2.2. Get non-speculative ancestors of head block, up to the number necessary to satisfy limit.
        let mut cursor = highest_committed_block;
        while limit - res.len() > 0 {
            let next_hash = cursor.justify.block_hash;
            let cursor_height = cursor.height;
            res.push(cursor);
            if cursor_height == 0 {
                break
            }

            cursor = block_tree.get_block_by_hash(&next_hash).unwrap();
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

// Possibly unintuitive behavior: The returned chain is ordered from GREATER block height to SMALLER block height.
fn get_chain_between_speculative_block_and_highest_committed_block(block_tree: &BlockTreeSnapshot, head_block_hash: &BlockHash) -> Vec<Block> {
    let mut res = Vec::new();

    let top_block = block_tree.get_top_block();
    let highest_committed_block = block_tree.get_highest_committed_block();
    let mut cursor = top_block.justify.block_hash;
    res.push(top_block);

    while cursor != highest_committed_block.hash {
        let block = block_tree.get_block_by_hash(&cursor).unwrap();
        cursor = block.justify.block_hash;
        res.push(block);
    } 

    res
}

pub(crate) struct SyncModeClient(reqwest::blocking::Client);

impl SyncModeClient {
    pub fn new(request_timeout: Duration) -> SyncModeClient {
        let reqwest_client = reqwest::blocking::Client::builder().timeout(request_timeout).build()
            .expect("Programming or Configuration error: failed to open a HTTP client for SyncModeClient.");
        SyncModeClient(reqwest_client)
    }

    pub fn get_blocks_from_tail(&self, tail_block_hash: &BlockHash, limit: usize, participant_ip_addr: &IpAddr) -> Result<Vec<Block>, GetBlocksFromTailError> {
        let url = format!(
            "http://{}/blocks?hash={}&anchor=tail&limit={}&speculate=true",
            participant_ip_addr,
            Base64URL::encode(tail_block_hash).deref(),
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

