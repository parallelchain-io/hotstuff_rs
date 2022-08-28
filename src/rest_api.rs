use std::net::SocketAddr;
use tokio;
use serde;
use warp::hyper::StatusCode;
use warp::{self, http, Filter};
use crate::msg_types::{Node, NodeHash, SerDe};
use crate::{NodeTreeSnapshotFactory, NodeTreeSnapshot};
use crate::config::NodeTreeApiConfig;

struct Server(tokio::runtime::Runtime);

impl Server {
    fn start(node_tree: NodeTreeSnapshotFactory, config: NodeTreeApiConfig) -> Server {
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

    async fn handle_get_nodes(node_tree: NodeTreeSnapshot, mut params: GetNodesParams) -> impl warp::Reply {
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
            } => match get_nodes_forwards(&node_tree, &hash, limit, speculate) {
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

fn get_nodes_forwards(node_tree: &NodeTreeSnapshot, start_node_hash: &NodeHash, limit: usize, speculate: bool) -> Option<Vec<Node>> {
    todo!()
}

fn get_chain_between_highest_committed_node_and_top_node(node_tree: &NodeTreeSnapshot) -> Vec<Node> {
    todo!()
}

