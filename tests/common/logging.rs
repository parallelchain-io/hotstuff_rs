use std::time::SystemTime;

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};

use super::verifying_key_bytes::VerifyingKeyBytes;

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

pub(crate) fn first_seven_base64_chars(bytes: &[u8]) -> String {
    let encoded = STANDARD_NO_PAD.encode(bytes);
    if encoded.len() > 7 {
        encoded[0..7].to_string()
    } else {
        encoded
    }
}

fn secs_since_unix_epoch() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Event occured before the Unix Epoch.")
        .as_secs()
}
