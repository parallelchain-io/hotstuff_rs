/// Defines types and encodings that are used in messages exchanged between Participants. The wire-encodings of these types are an
/// integral part of the protocol, and all Participant implementations that wish to communicate with the implementation in this crate
/// needs to encode these types in the exact same way.
pub mod messages;

/// Defines *additional* types that the Block Tree's storage implementation needs to store to support its functionality, but which are
/// not sent over the wire and therefore are not part of the protocol. An example functionality of the Block Tree that requires these
/// additional types is getting the child of a Block, since a msg_types::Block only stores a pointer to its parent in the form of 
/// `justify.block_hash`.
pub mod stored;

/// Defines types that give a cryptographic identity to every Participant. This chiefly includes secret keys, public keys, and signatures.
pub mod identity;
