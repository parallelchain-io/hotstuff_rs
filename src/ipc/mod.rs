/// The IPC module handles HotStuff-rs' interactions with other Participants in Progress Mode. Inter-process
/// interactions in Sync Mode are handled in `crate::sync_mode::ipc`.

pub(crate) mod handle;

pub(crate) mod connection_set;

pub(crate) mod establisher;

pub(crate) mod stream;

// Re-exports
pub(in crate::ipc) use connection_set::ConnectionSet;
pub(in crate::ipc) use establisher::*;
pub(in crate::ipc) use stream::*;
pub(crate) use handle::*;
