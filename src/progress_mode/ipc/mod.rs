/// The IPC module handles HotStuff-rs' interactions with other Participants in Progress Mode. Inter-process
/// interactions in Sync Mode are handled in `crate::sync_mode::ipc`.

pub(crate) const NET_LATENCY: std::time::Duration = todo!();

pub(crate) mod handle;
pub(crate) use handle::Handle;

pub(crate) mod connection_set;
pub(in crate::progress_mode::ipc) use connection_set::ConnectionSet;

pub(crate) mod establisher;
pub(in crate::progress_mode::ipc) use establisher::*;

pub(crate) mod stream;
pub(in crate::progress_mode::ipc) use stream::*;
