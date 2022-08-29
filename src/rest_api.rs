use std::net::SocketAddr;
use std::cmp::min;
use tokio;
use serde;
use warp::hyper::StatusCode;
use warp::{self, http, Filter};
use crate::msg_types::{Node, NodeHash, SerDe};
use crate::node_tree::{NodeTreeSnapshotFactory, NodeTreeSnapshot, ChildrenNotYetCommittedError};
use crate::config::NodeTreeApiConfig;

struct Server(tokio::runtime::Runtime);

impl Server {
    fn start(node_tree: NodeTreeSnapshotFactory, config: NodeTreeApiConfig) -> Server {
        // Endpoints and their behavior are documented in node_tree/rest_api.md

        // GET /nodes
        let get_nodes = warp::path("nodes")
            .and(warp::query::<GetNodesParams>())
            .then(move |nodes_params| Self::handle_get_nodes(node_tree.snapshot(), nodes_params));

        let server = warp::serve(get_nodes);
        let listening_socket_addr = SocketAddr::new(config.listening_addr, config.listening_port); 
        let task = server.run(listening_socket_addr);

        let runtime = tokio::runtime::Runtime::new()
            .expect("Programming or Configuration error: fail to create Tokio runtime.");
        let _ = runtime.spawn(task);

        Server(runtime)
    }

    async fn handle_get_nodes<'a>(node_tree: NodeTreeSnapshot<'a>, mut params: GetNodesParams) -> impl warp::Reply {
        // Apply default values if parameter is not assigned to.
        let _ = params.direction.get_or_insert(GetNodesAnchorEnd::Backward);
        let _ = params.limit.get_or_insert(1);
        let _ = params.speculate.get_or_insert(false);

        match params {
            GetNodesParams {
                hash,
                anchor_end: Some(GetNodesAnchorEnd::Head),
                limit,
                speculate,
            } => match get_nodes_up_to_head(&node_tree, &hash, limit.unwrap(), speculate.unwrap()) {
                Some(nodes) => http::Response::builder().status(StatusCode::OK).body(nodes.serialize()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            },
            GetNodesParams {
                hash,
                anchor_end: Some(GetNodesAnchorEnd::Tail),
                limit,
                speculate,
            } => match get_nodes_from_tail(&node_tree, &hash, limit.unwrap(), speculate.unwrap()) {
                Some(nodes) => http::Response::builder().status(StatusCode::OK).body(nodes.serialize()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            }, 
            _ => unreachable!()
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GetNodesParams {
    hash: NodeHash,
    anchor_end: Option<GetNodesAnchorEnd>,
    limit: Option<usize>,
    speculate: Option<bool>,
}

enum GetNodesAnchorEnd {
    Head,
    Tail,
}

fn get_nodes_from_tail(node_tree: &NodeTreeSnapshot, tail_node_hash: &NodeHash, limit: usize, speculate: bool) -> Option<Vec<Node>> {
    let res = Vec::with_capacity(limit);

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
        let uncommitted_nodes = get_chain_between_speculative_node_and_highest_committed_node(&node_tree, &top_node.hash).iter().rev();
        res.extend(&uncommitted_nodes[..min(limit - res.len(), uncommitted_nodes.len())]);
    }

    Some(res)
}

fn get_nodes_up_to_head(node_tree: &NodeTreeSnapshot, head_node_hash: &NodeHash, limit: usize, speculate: bool) -> Option<Vec<Node>> {
    let res = Vec::with_capacity(limit);

    // 1. Get head node.
    let head_node = node_tree.get_node(head_node_hash)?;
    res.push(head_node);
    if limit == 1 {
        // 1.1. If only one node is requested, return immediately.
        return Some(res)
    }

    // 2. Check whether head node is speculative.
    let highest_committed_node = node_tree.get_highest_committed_node();
    if head_node.height > highest_committed_node {
        // Start node IS speculative. 
        if !speculate {
            return None
        }

        // 2.1. Get speculative ancestors of head node.
        let speculative_ancestors = get_chain_between_speculative_node_and_highest_committed_node(node_tree, &head_node_hash).iter().rev().collect();
        res.append(speculative_ancestors[min(limit - res.len(), speculative_ancestors.len())]);

        // 2.2. Get non-speculative ancestors of head node, up to the number necessary to satisfy limit.
        let mut cursor = highest_committed_node;
        while limit - res.len() > 0 {
            res.push(cursor);
            if cursor.height == 0 {
                break
            }

            cursor = node_tree.get_node(&cursor.justify.node_hash);
        }
    } else {
        // Start node IS NOT speculative.

        // 2.1. Get ancestors of head node, up to the number necessary to satisfy limit.
        let mut cursor = node_tree.get_node(&head_node.justify.node_hash);
        while limit - res.len() > 0 {
            res.push(cursor);
            if cursor.height == 0 {
                break
            }

            cursor = node_tree.get_node(&cursor.justify.node_hash);
        }

    }

    Some(res)
}

// Possibly unintuitive behavior: The returned chain is ordered from GREATER node height to SMALLER node height.
fn get_chain_between_speculative_node_and_highest_committed_node(node_tree: &NodeTreeSnapshot, head_node_hash: &NodeHash) -> Vec<Node> {
    let res = Vec::new();

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
