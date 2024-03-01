/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for the [Keypair] type as an object used to sign messages and access the public key.

use ed25519_dalek::{SigningKey, VerifyingKey};

use super::basic::SignatureBytes;

/// A wrapper around [SigningKey](ed25519_dalek::SigningKey) which implements a [convenience method](Keypair::sign) for creating signatures.
pub(crate) struct Keypair(pub(crate) SigningKey);

impl Keypair {
    pub(crate) fn new(signing_key: SigningKey) -> Keypair {
        Keypair(signing_key)
    }
    
    /// Convenience method for creating signatures over values or messages represented as vectors of bytes.
    pub(crate) fn sign(&self, message: Vec<u8>) -> SignatureBytes {
        self.0.sign(message).to_bytes()
    }

    pub(crate) fn public(&self) -> VerifyingKey {
        self.0.verifying_key()
    }
}

