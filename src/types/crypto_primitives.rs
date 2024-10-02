/*
    Copyright © 2024, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Cryptographic primitives.
//!
//! Most of the types in this module are re-exports from other libraries, namely [`sha2`] and
//! [`ed25519_dalek`].

use super::data_types::SignatureBytes;

// re-exports below.
pub use sha2::Digest;
pub use sha2::Sha256 as CryptoHasher;

pub use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// A facade around [`SigningKey`] that implements method for [`sign`](Self::sign)-ing messages as well
/// as a getter for the [`public`](Self::public) key associated with the signing key.
#[derive(Clone)]
pub(crate) struct Keypair(pub(crate) SigningKey);

impl Keypair {
    /// Create a `Keypair` that wraps over `signing_key`.
    pub(crate) fn new(signing_key: SigningKey) -> Keypair {
        Keypair(signing_key)
    }

    /// Sign an arbitrary `message` with the `Keypair`.
    pub(crate) fn sign(&self, message: &Vec<u8>) -> SignatureBytes {
        SignatureBytes::new(self.0.sign(message).to_bytes())
    }

    /// Get the `VerifyingKey` of this `Keypair`.
    pub(crate) fn public(&self) -> VerifyingKey {
        self.0.verifying_key()
    }
}
