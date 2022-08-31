use std::net::{SocketAddr, IpAddr};
use std::cmp::min;
use std::ops::Deref;
use std::time::Duration;
use tokio;
use serde;
use warp::hyper::StatusCode;
use warp::{self, http, Filter};
use reqwest;
use pchain_types::Base64URL;
use crate::msg_types::{Node, NodeHash, SerDe, NODE_HASH_LEN};
use crate::node_tree::{NodeTreeSnapshotFactory, NodeTreeSnapshot, ChildrenNotYetCommittedError, self};
use crate::config::NodeTreeApiConfig;

struct Server(tokio::runtime::Runtime);

impl Server {
    fn start(node_tree_snapshot_factory: NodeTreeSnapshotFactory, config: NodeTreeApiConfig) -> Server {
        // Endpoints and their behavior are documented in node_tree/rest_api.md

        // GET /nodes
        let get_nodes = warp::path("nodes")
            .and(warp::query::<GetNodesParams>())
            .then(move |nodes_params| Self::handle_get_nodes(node_tree_snapshot_factory.clone(), nodes_params));

        let server = warp::serve(get_nodes);
        let listening_socket_addr = SocketAddr::new(config.listening_addr, config.listening_port); 
        let task = server.run(listening_socket_addr);

        let runtime = tokio::runtime::Runtime::new()
            .expect("Programming or Configuration error: fail to create Tokio runtime.");
        let _ = runtime.spawn(task);

        Server(runtime)
    }

    async fn handle_get_nodes<'a>(node_tree_snapshot_factory: NodeTreeSnapshotFactory, mut params: GetNodesParams) -> impl warp::Reply {
        // Interpret query parameters
        let hash = match Base64URL::decode(&params.hash) {
            Err(_) => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
            Ok(hash) => match hash.try_into() {
                Ok(hash) => hash,
                Err(_) => return http::Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
            }
        };
        let anchor = match params.anchor {
            None => GetNodesAnchor::Head,
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

        let node_tree_snapshot = node_tree_snapshot_factory.snapshot();

        match anchor {
            GetNodesAnchor::Head => match get_nodes_up_to_head(&node_tree_snapshot, &hash, limit, speculate) {
                Some(nodes) => http::Response::builder().status(StatusCode::OK).body(nodes.serialize()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            },
            GetNodesAnchor::Tail => match get_nodes_from_tail(&node_tree_snapshot, &hash, limit, speculate) {
                Some(nodes) => http::Response::builder().status(StatusCode::OK).body(nodes.serialize()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            }, 
            _ => unreachable!()
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GetNodesParams {
    hash: String,
    anchor: Option<GetNodesAnchor>,
    limit: Option<usize>,
    speculate: Option<bool>,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum GetNodesAnchor {
    Head,
    Tail,
}

fn get_nodes_from_tail(node_tree: &NodeTreeSnapshot, tail_node_hash: &NodeHash, limit: usize, speculate: bool) -> Option<Vec<Node>> {
    let mut res = Vec::with_capacity(limit);

    // 1. Get tail node.
    let tail_node = node_tree.get_node(tail_node_hash)?;
    let mut cursor = tail_node.hash;
    res.push(tail_node);

    // 2. Walk through tail node's descendants until limit is satisfied or we hit uncommitted nodes.
    while res.len() < limit {
        let child = match node_tree.get_child(&cursor) {
            Ok(node) => node,
            Err(ChildrenNotYetCommittedError) => break,
        };
        cursor = child.hash;
        res.push(child);
    }

    // 3. If limit is not yet satisfied and we are allowed to speculate, get speculative nodes.
    if res.len() < limit && speculate {
        let top_node = node_tree.get_top_node();
        // 3.1. Reverse uncommitted nodes so that nodes with lower heights appear first.
        let uncommitted_nodes: Vec<Node> = get_chain_between_speculative_node_and_highest_committed_node(&node_tree, &top_node.hash).into_iter().rev().collect();
        res.extend_from_slice(&uncommitted_nodes[..min(limit - res.len(), uncommitted_nodes.len())]);
    }

    Some(res)
}

fn get_nodes_up_to_head(node_tree: &NodeTreeSnapshot, head_node_hash: &NodeHash, limit: usize, speculate: bool) -> Option<Vec<Node>> {
    let mut res = Vec::with_capacity(limit);

    // 1. Get head node.
    let head_node = node_tree.get_node(head_node_hash)?;
    let head_node_parent_hash = head_node.justify.node_hash;
    let head_node_height = head_node.height;
    res.push(head_node);
    if limit == 1 {
        // 1.1. If only one node is requested, return immediately.
        return Some(res)
    }

    // 2. Check whether head node is speculative.
    let highest_committed_node = node_tree.get_highest_committed_node();
    if head_node_height > highest_committed_node.height {
        // Start node IS speculative. 
        if !speculate {
            return None
        }

        // 2.1. Get speculative ancestors of head node.
        let speculative_ancestors: Vec<Node> = get_chain_between_speculative_node_and_highest_committed_node(node_tree, &head_node_hash).into_iter().rev().collect();
        res.extend_from_slice(&speculative_ancestors[..min(limit - res.len(), speculative_ancestors.len())]);

        // 2.2. Get non-speculative ancestors of head node, up to the number necessary to satisfy limit.
        let mut cursor = highest_committed_node;
        while limit - res.len() > 0 {
            let next_hash = cursor.justify.node_hash;
            let cursor_height = cursor.height;
            res.push(cursor);
            if cursor_height == 0 {
                break
            }

            cursor = node_tree.get_node(&next_hash).unwrap();
        }
    } else {
        // Start node IS NOT speculative.

        // 2.1. Get ancestors of head node, up to the number necessary to satisfy limit.
        let mut cursor = node_tree.get_node(&head_node_parent_hash).unwrap();
        while limit - res.len() > 0 {
            let next_hash = cursor.justify.node_hash;
            let cursor_height = cursor.height;
            res.push(cursor);
            if cursor_height == 0 {
                break
            }

            cursor = node_tree.get_node(&next_hash).unwrap();
        }

    }

    Some(res)
}

// Possibly unintuitive behavior: The returned chain is ordered from GREATER node height to SMALLER node height.
fn get_chain_between_speculative_node_and_highest_committed_node(node_tree: &NodeTreeSnapshot, head_node_hash: &NodeHash) -> Vec<Node> {
    let mut res = Vec::new();

    let top_node = node_tree.get_top_node();
    let highest_committed_node = node_tree.get_highest_committed_node();
    let mut cursor = top_node.justify.node_hash;
    res.push(top_node);

    while cursor != highest_committed_node.hash {
        let node = node_tree.get_node(&cursor).unwrap();
        cursor = node.justify.node_hash;
        res.push(node);
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

    pub fn get_nodes_from_tail(&self, tail_node_hash: &NodeHash, limit: usize, participant_ip_addr: &IpAddr) -> Result<Vec<Node>, GetNodesFromTailError> {
        let url = format!(
            "http://{}/nodes?hash={}&anchor=end&limit={}&speculate=true",
            participant_ip_addr,
            Base64URL::encode(tail_node_hash).deref(),
            limit,
        );
        let resp = self.0.get(url).send().map_err(Self::convert_request_error)?;
        let resp_bytes = resp.bytes().map_err(|_| GetNodesFromTailError::BadResponse)?; 
        let (_, nodes) = Vec::<Node>::deserialize(&resp_bytes).map_err(|_| GetNodesFromTailError::BadResponse)?;

        Ok(nodes)
    }

    fn convert_request_error(e: reqwest::Error) -> GetNodesFromTailError {
        if e.is_timeout() {
            GetNodesFromTailError::RequestTimedOut
        } else if e.is_connect() {
            GetNodesFromTailError::FailToConnectToServer
        } else if let Some(status) = e.status() {
            if status == StatusCode::NOT_FOUND {
                GetNodesFromTailError::TailNodeNotFound
            } else {
                GetNodesFromTailError::BadResponse
            }
        } else {
            GetNodesFromTailError::BadResponse
        }
    }
}

pub(crate) enum GetNodesFromTailError {
    FailToConnectToServer,
    RequestTimedOut,
    TailNodeNotFound,
    BadResponse,
}

