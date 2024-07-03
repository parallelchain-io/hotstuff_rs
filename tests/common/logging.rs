//! Utilities for printing human-readable messages from integration tests.

use std::time::SystemTime;

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};

use super::verifying_key_bytes::VerifyingKeyBytes;

/// Log a `msg` with a prefix indicating the current time and (optionally) the `verifying_key` of the
/// replica which emitted the log.
pub(crate) fn log_with_context(verifying_key: Option<VerifyingKeyBytes>, msg: &str) {
    println!(
        "[{}] [{}] {}",
        secs_since_unix_epoch(),
        match verifying_key {
            Some(verifying_key) => first_seven_base64_chars(&verifying_key),
            None => String::from("-------"),
        },
        msg,
    )
}

/// Encode `bytes` into Base64 and return the first up to 7 characters in the encoding as a String.
pub(crate) fn first_seven_base64_chars(bytes: &[u8]) -> String {
    let encoded = STANDARD_NO_PAD.encode(bytes);
    if encoded.len() > 7 {
        encoded[0..7].to_string()
    } else {
        encoded
    }
}

/// Get the number of seconds that has elapsed since the UNIX epoch (1970-01-01 00-00-00 UTC).
fn secs_since_unix_epoch() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Event occured before the Unix Epoch.")
        .as_secs()
}
