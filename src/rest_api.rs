use std::convert::identity;
use std::net::SocketAddr;
use std::ops::Deref;
use borsh::{BorshSerialize, BorshDeserialize};
use tokio;
use serde;
use warp::hyper::StatusCode;
use warp::{self, http, Filter};
use reqwest;
use hotstuff_rs_types::identity::{PublicKeyBytes, ParticipantSet};
use hotstuff_rs_types::messages::{Block, BlockHeight};
use hotstuff_rs_types::base64url::Base64URL;
use crate::block_tree::BlockTreeSnapshotFactory;
use crate::config::SyncModeNetworkingConfig;

pub(crate) struct Server(tokio::runtime::Runtime);

impl Server {
    pub(crate) fn start(block_tree_snapshot_factory: BlockTreeSnapshotFactory, config: SyncModeNetworkingConfig) -> Server {
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
        let listening_socket_addr = SocketAddr::new(config.block_tree_api_listening_addr, config.block_tree_api_listening_port); 
        let task = server.run(listening_socket_addr);

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
            GetBlocksAnchor::Head => match block_tree_snapshot.get_blocks_up_to_head(&hash, limit, speculate) {
                Some(blocks) => http::Response::builder().status(StatusCode::OK).body(blocks.try_to_vec().unwrap()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            },
            GetBlocksAnchor::Tail => match block_tree_snapshot.get_blocks_from_tail(&hash, limit, speculate) {
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
            Some(value) => http::Response::builder().status(StatusCode::OK).body(value),
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

pub(crate) struct SyncModeClient {
    reqwest_client: reqwest::blocking::Client,
    config: SyncModeNetworkingConfig,
    participant_set: ParticipantSet,
}

impl SyncModeClient {
    pub fn new(config: SyncModeNetworkingConfig, participant_set: ParticipantSet) -> SyncModeClient {
        let reqwest_client = reqwest::blocking::Client::builder().timeout(config.request_timeout).build()
            .expect("Programming or Configuration error: failed to open a HTTP client for SyncModeClient.");

        SyncModeClient {
            reqwest_client,
            config,
            participant_set,
        }
    }

    fn convert_request_error(e: reqwest::Error) -> GetBlocksFromTailError {
        if e.is_timeout() {
            GetBlocksFromTailError::RequestTimedOut
        } else if e.is_connect() {
            GetBlocksFromTailError::FailToConnectToServer
        } else {
            GetBlocksFromTailError::BadResponse
        }
    }
}

impl AbstractSyncModeClient for SyncModeClient {
    fn get_blocks_from_tail(&self, participant: PublicKeyBytes, tail_block_height: BlockHeight, limit: usize) -> Result<Vec<Block>, GetBlocksFromTailError> {
        let participant_ip_addr = self.participant_set.get(&participant).unwrap();

        let url = format!(
            "http://{}:{}/blocks?height={}&anchor=Tail&limit={}&speculate=true",
            participant_ip_addr,
            self.config.block_tree_api_listening_port,
            tail_block_height,
            limit,
        );
        let resp = self.reqwest_client.get(url).send().map_err(Self::convert_request_error)?;
        match resp.status() {
            StatusCode::OK => {
                let resp_bytes = resp.bytes().map_err(|_| GetBlocksFromTailError::BadResponse)?; 
                let blocks = Vec::<Block>::deserialize(&mut &resp_bytes[..]).map_err(|_| GetBlocksFromTailError::BadResponse)?;
                Ok(blocks)
            },
            StatusCode::NOT_FOUND => Err(GetBlocksFromTailError::TailBlockNotFound),
            _ => Err(GetBlocksFromTailError::BadResponse),
        }
    }
}

pub(crate) enum GetBlocksFromTailError {
    FailToConnectToServer,
    RequestTimedOut,
    TailBlockNotFound,
    BadResponse,
}

pub(crate) trait AbstractSyncModeClient {
    fn get_blocks_from_tail(&self, participant: PublicKeyBytes, tail_block_height: BlockHeight, limit: usize) -> Result<Vec<Block>, GetBlocksFromTailError>;
}
