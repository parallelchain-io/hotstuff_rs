/// Defines the HotStuff struct, your Apps' point of entry into HotStuff-rs' threads and business logic. Calling the struct's `start`
/// associated function starts up HotStuff-rs's Consensus State Machine.
pub mod hotstuff;

/// Defines the App trait, which your types are required to implement in order to serve as HotStuff-rs' deterministic state transition
/// function. The Ethereum Virtual Machine (EVM), for example, is a classic example of a deterministic state transition function which
/// could implement the App trait.
pub mod app;

/// Defines structures whose fields configure the behavior of the protocol Participant. This includes the pace at which consensus should
/// proceed, how Participants should contact each other, etc. If you are building a distributed system using HotStuff-rs, you'd want to
/// tweak these knobs to modify the system's behavior to meet your needs.
pub mod config;

/// Defines the 'Block Tree', the object of state machine replication. Consensus works to extend the Block Tree with new Blocks, causing
/// the distributed state machine's World State to evolve over time. The 'Block Tree' is a generalization of the concept of a 'blockchain'.
pub mod block_tree;

/// Defines types and encodings that are used in messages exchanged between Participants. The wire-encodings of these types are an
/// integral part of the protocol, and all Participant implementations that wish to communicate with the implementation in this crate
/// needs to encode these types in the exact same way.
pub mod msg_types;

/// Defines *additional* types that the Block Tree's storage implementation needs to store to support its functionality, but which are
/// not sent over the wire and therefore are not part of the protocol. An example functionality of the Block Tree that requires these
/// additional types is getting the child of a Block, since a msg_types::Block only stores a pointer to its parent in the form of 
/// `justify.block_hash`.
pub mod stored_types;

/// Defines an HTTP server that serves the 'Block Tree HTTP API' (endpoints documented in '/docs'). The `GET /blocks` route of this API
/// is used by the protocol state machine's 'Sync Mode' to catch-up lagging Participants to the global head of the Block Tree, and therefore
/// is a required part of the protocol. The rest of the routes are optional.
pub(crate) mod rest_api;

/// Defines types that give a cryptographic identity to every Participant. This chiefly includes secret keys, public keys, and signatures.
pub(crate) mod identity;

/// Defines HotStuff-rs' Consensus State Machine, the component which actually drives the growth of the Block Tree through Byzantine Fault
/// Tolerant consensus. 
pub(crate) mod state_machine;

/// Defines types and routines that interact with network I/O to enable this Participant to contact other Participants in Progress Mode.
/// Sync Mode networking involves types defined in the rest_api module.
pub(crate) mod ipc;
