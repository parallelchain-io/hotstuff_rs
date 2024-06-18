use std::{io, sync::Once, thread, time::SystemTime};

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use log::LevelFilter;

static LOGGER_INIT: Once = Once::new();

// Set up a logger that logs all log messages with level Trace and above.
pub(crate) fn setup_logger(level: LevelFilter) {
    LOGGER_INIT.call_once(|| {
        fern::Dispatch::new()
            .format(|out, message, record| {
                out.finish(format_args!(
                    "[{:?}][{}] {}",
                    thread::current().id(),
                    record.level(),
                    message
                ))
            })
            .level(level)
            .chain(io::stdout())
            .apply()
            .unwrap();
    })
}

// Get a more readable representation of a bytesequence by base64-encoding it and taking the first 7 characters.
pub(crate) fn first_seven_base64_chars(bytes: &[u8]) -> String {
    let encoded = STANDARD_NO_PAD.encode(bytes);
    if encoded.len() > 7 {
        encoded[0..7].to_string()
    } else {
        encoded
    }
}

pub(crate) fn secs_since_unix_epoch(timestamp: SystemTime) -> u64 {
    timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Event occured before the Unix Epoch.")
        .as_secs()
}
