/// The IPC module handles HotStuff-rs' interactions with other Participants in Progress Mode. Inter-process
/// interactions in Sync Mode are handled in `node_tree::api`.

/// Defines Handle, a struct which exposes methods that offer *blocking* reads and *non-blocking* writes for
/// the Worker thread's Sync Mode.
pub(crate) mod handle;
pub(crate) use handle::Handle;

/// Defines Manager, a struct which implements the blocking reads and non-blocking writes functionality that
/// Handle exposes. In 2 words, it is Handle's backend. 
pub(crate) mod manager;
pub(crate) use manager::Manager;

/// Defines RwTcpStream, a synchronization wrapper around net::TcpStream with desirable thread-safety properties. 
pub(crate) mod rw_tcpstream;
pub(crate) use rw_tcpstream::RwTcpStream;

/// Defines ConnectionSet, a type alias of HashSet<PublicAddress, Arc<RwTcpStream>> that is frequently referenced
/// throughout the IPC module. 
pub(crate) mod connection_set;
pub(crate) use connection_set::ConnectionSet;