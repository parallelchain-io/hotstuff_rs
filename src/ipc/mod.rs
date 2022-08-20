/// The IPC module handles HotStuff-rs' interactions with other Participants in Progress Mode. Inter-process
/// interactions in Sync Mode are handled in `node_tree::api`.

/// Defines Handle, a struct which exposes methods that offer *blocking* reads and *non-blocking* writes for
/// the Worker thread's Sync Mode.
pub(crate) mod handle;
pub(crate) use handle::Handle;

/// Defines Manager, a struct which implements the blocking reads and non-blocking writes functionality that
/// Handle exposes. In 2 words, it is Handle's backend. 
pub(crate) mod conn_establisher;
pub(crate) use conn_establisher::ConnEstablisher;

pub(crate) mod stream;
pub(crate) use stream::Stream;

/// Defines ConnectionSet, a type alias of HashSet<PublicAddress, Arc<RwTcpStream>> that is frequently referenced
/// throughout the IPC module. 
pub(crate) mod conn_set;
pub(crate) use conn_set::ConnSet;