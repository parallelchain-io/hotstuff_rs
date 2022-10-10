//! HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
//! 1. Guaranteed Safety and Liveness in the face of up to 1/3rd of Voting Power being Byzantine at any given moment,
//! 2. A small API (`App`) for plugging in arbitrary state machine-based applications like blockchains, and
//! 3. Well-documented, 'obviously correct' source code, designed for easy analysis and extension.


/// Defines the HotStuff struct, your Apps' point of entry into HotStuff-rs' threads and business logic. Calling the struct's `start`
/// associated function starts up HotStuff-rs's Consensus State Machine.
pub mod hotstuff;

/// Defines the [App](app::App) trait, which your types are required to implement in order to serve as HotStuff-rs' deterministic state 
/// transition function. The Ethereum Virtual Machine (EVM), for example, is a classic example of a deterministic state transition function
/// which could implement the App trait.
pub mod app;

/// Defines structures whose fields configure the behavior of the protocol Participant. This includes the pace at which consensus should
/// proceed, how Participants should contact each other, etc. If you are building a distributed system using HotStuff-rs, you would want to
/// tweak these knobs to modify the system's behavior to meet your needs.
pub mod config;

/// Defines the 'Block Tree', the object of state machine replication. Consensus works to extend the Block Tree with new Blocks, causing
/// the distributed state machine's Storage to evolve over time. The 'Block Tree' is a generalization of the concept of a 'blockchain'.
pub mod block_tree;

/// Defines an HTTP server that serves the 'Block Tree HTTP API' (endpoints documented in '/docs'). The `GET /blocks` route of this API
/// is used by the protocol state machine's 'Sync Mode' to catch-up lagging Participants to the global head of the Block Tree, and therefore
/// is a required part of the protocol. The rest of the routes are optional.
pub(crate) mod rest_api;

/// Defines HotStuff-rs' Algorithm State Machine, the component which actually drives the growth of the Block Tree through Byzantine Fault
/// Tolerant consensus. 
pub(crate) mod algorithm;

/// Defines types and routines that interact with network I/O to enable this Participant to contact other Participants in Progress Mode.
/// Sync Mode networking involves types defined in the rest_api module.
pub(crate) mod ipc;
