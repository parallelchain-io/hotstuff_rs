/// The IPC module handles HotStuff-rs' interactions with other Participants in Progress Mode. Inter-process
/// interactions in Sync Mode are handled in `node_tree::api`.

pub(crate) mod handle;
pub(crate) use handle::Handle;

pub(crate) mod managed_conn_set;
pub(in crate::ipc) use managed_conn_set::ManagedConnSet;

pub(crate) mod conn_set;
pub(in crate::ipc) use conn_set::ConnSet;

pub(crate) mod stream;
pub(in crate::ipc) use stream::Stream;
