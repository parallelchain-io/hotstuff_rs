use std::net::SocketAddr;
use tokio;
use serde;
use warp::hyper::StatusCode;
use warp::{self, http, Filter};
use crate::msg_types::{Node, NodeHash, SerDe};
use crate::node_tree::{NodeTree};
use crate::config::NodeTreeApiConfig;

struct Server(tokio::runtime::Runtime);

impl Server {
    fn start(node_tree: NodeTree, config: NodeTreeApiConfig) -> Server {
        // Endpoints and their behavior are documented in node_tree/rest_api.md

        // GET /nodes
        let get_nodes = warp::path("nodes")
            .and(warp::query::<GetNodesParams>())
            .then(move |nodes_params| Self::handle_get_nodes(node_tree.clone(), nodes_params));

        let server = warp::serve(get_nodes);
        let listening_socket_addr = SocketAddr::new(config.listening_addr, config.listening_port); 
        let task = server.run(listening_socket_addr);

        let runtime = tokio::runtime::Runtime::new()
            .expect("Programming or Configuration error: fail to create Tokio runtime.");
        let _ = runtime.spawn(task);

        Server(runtime)
    }

    async fn handle_get_nodes(node_tree: NodeTree, mut params: GetNodesParams) -> impl warp::Reply {
        // Apply default values if parameter is not assigned to.
        let _ = params.direction.get_or_insert(GetNodesDirections::Backward);
        let _ = params.limit.get_or_insert(1);
        let _ = params.speculate.get_or_insert(false);

        match params {
            GetNodesParams {
                height: None,
                hash: Some(hash),
                direction: Some(GetNodesDirections::Forward),
                limit: Some(limit),
                speculate: Some(speculate),
            } => match node_tree.get_nodes_forwards(&hash, limit, speculate) {
                Some(nodes) => http::Response::builder().status(StatusCode::OK).body(nodes.serialize()),
                None => http::Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            },
            _ => todo!()
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GetNodesParams {
    height: Option<usize>,
    hash: Option<NodeHash>,
    direction: Option<GetNodesDirections>,
    limit: Option<usize>,
    speculate: Option<bool>,
}

enum GetNodesDirections {
    Forward,
    Backward, 
}

impl NodeTree {
    fn get_nodes_forwards(&self, from_node: &NodeHash, limit: usize, speculate: bool) -> Option<Vec<Node>> {
        // 1. Collect non-speculative descendants of `from_node`.
        let from_node = self.get_node(&from_node)?;
        let mut nodes: Vec<Node> = Vec::with_capacity(limit);
        nodes.push(from_node.into());
        while nodes.len() < limit {
            let child = match self.get_child(&nodes[nodes.len() - 1].hash()) {
                Ok(node) => node,
                Err(ChildNotYetCommittedError) => break,
            };
            nodes.push(child.into()); 
        }

        // 2. If limit is not yet exhausted, and speculate is true, collect speculative descendants.
        if nodes.len() < limit && speculate {
            loop {
                match self.get_descendants_by_backtracking(&nodes[nodes.len() - 1], limit) {
                    Ok(speculative_descendants) => {
                        nodes.extend_from_slice(&speculative_descendants);
                        break
                    },
                    Err(BranchAbandonedError) => {
                        continue
                    }
                }
            }
        }

        return Some(nodes)
    }

    // # Panics
    // if limit > 3.
    fn get_descendants_by_backtracking(&self, ancestor: &Node, limit: usize) -> Result<Vec<Node>, BranchAbandonedError> {
        if limit > 3 {
            panic!("Programming error: called get_descendants_by_backtracking with limit > 3.")
        }

        // 1. Position the cursor on the Node with height == ancestor.get_height() + limit.
        let mut cursor = self.get_node_with_generic_qc();
        while self.get_height(&cursor.hash()).ok_or(BranchAbandonedError)? 
            != self.get_height(&ancestor.hash()).ok_or(BranchAbandonedError)? + limit {
            cursor = self.get_parent(&cursor.hash()).map_err(|_| BranchAbandonedError)?;
        }

        // 2. Collect the `limit` Nodes lying in the chain between ancestor and the Node stored in
        // `cursor` at the end of step 1.
        let mut res = Vec::new();
        while self.get_height(&cursor.hash()).ok_or(BranchAbandonedError)?
            != self.get_height(&ancestor.hash()).ok_or(BranchAbandonedError)? {
            res.push(cursor.into());
            cursor = self.get_parent(&cursor.hash()).map_err(|_| BranchAbandonedError)?;
        }

        // 3. Reverse res.
        let res = res.into_iter().rev().collect();

        Ok(res)
    }
}

struct BranchAbandonedError;
