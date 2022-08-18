use std::sync::Mutex;
use std::net::{TcpStream, SocketAddr};
use std::io::{self, Read, Write};

/// RwTcpStream is a synchronization wrapper around RwTcpStream that allows at most a single thread to read from the
/// stream AND a single thread to write from the stream.
/// 
/// This exists in `ipc` for a particular reason: TcpStreams are safe to read from and write into concurrently (at
/// least in Linux), but are not safe to:
/// 1. Be read from multiple threads concurrently, or
/// 2. Be written from multiple threads concurrently.
/// 
/// For more context about the creation of this struct, visit: 
/// https://backend-gitlab.digital-transaction.net:3000/parallelchain/source/hotstuff_rs/-/issues/7
pub(crate) struct RwTcpStream {
    read_lock: Mutex<()>,
    write_lock: Mutex<()>,
    stream: TcpStream, 
}

impl RwTcpStream {
    pub fn new(stream: TcpStream) -> RwTcpStream {
        RwTcpStream {
            read_lock: Mutex::new(()),
            write_lock: Mutex::new(()),
            stream,
        }
    }

    pub fn read_exact(&self, buf: &mut [u8]) -> io::Result<()> {
        let _read_lock = self.read_lock.lock().unwrap();
        (&mut &self.stream).read_exact(buf)
    } 
    
    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        let _read_lock = self.read_lock.lock().unwrap();
        self.stream.peek(buf)
    }

    pub fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let _write_lock = self.write_lock.lock().unwrap();
        (&mut &self.stream).write_all(buf)
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }
}
