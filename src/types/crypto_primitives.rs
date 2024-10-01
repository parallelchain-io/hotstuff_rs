/*
    Copyright © 2024, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Cryptographic primitives.

use super::data_types::SignatureBytes;

// re-exports below.
pub use sha2::Sha256 as CryptoHasher;

pub use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// A wrapper around [SigningKey](ed25519_dalek::SigningKey) that implements a
/// [convenience method](Self::sign) for creating signatures as well as a [getter](Self::public) for the
/// public key.
#[derive(Clone)]
pub(crate) struct Keypair(pub(crate) SigningKey);

impl Keypair {
    pub(crate) fn new(signing_key: SigningKey) -> Keypair {
        Keypair(signing_key)
    }

    /// Convenience method for creating signatures over values or messages represented as vectors of bytes.
    pub(crate) fn sign(&self, message: &Vec<u8>) -> SignatureBytes {
        SignatureBytes::new(self.0.sign(message).to_bytes())
    }

    pub(crate) fn public(&self) -> VerifyingKey {
        self.0.verifying_key()
    }
}
