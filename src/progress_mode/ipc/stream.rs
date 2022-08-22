use std::io::{self, ErrorKind, Write, Read};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::sync::{mpsc, Mutex};
use std::net::{self, SocketAddr};
use std::time::Duration;
use crate::msg_types::{self, ConsensusMsg, SerDe, ViewNumber, Node, QuorumCertificate};

/// Stream is a wrapper around TcpStream which implements in-the-background reads and writes of ConsensusMsgs.
pub struct Stream {
    writer: thread::JoinHandle<()>,
    to_writer: mpsc::SyncSender<ConsensusMsg>,

    reader: thread::JoinHandle<()>,

    // Wrapped inside a Mutex so that Stream can be Sync. If Stream ends up never being shared between threads, this is
    // the overhead of locking is cheap enough that we deem it acceptable. 
    from_reader: Mutex<mpsc::Receiver<ConsensusMsg>>,

    peer_addr: SocketAddr,
}

impl Stream {
    const READ_TIMEOUT: Option<Duration> = Some(Duration::new(1000, 0));
    const WRITE_TIMEOUT: Option<Duration> = Some(Duration::new(1000, 0));
    const READER_CHANNEL_BUFFER_LENGTH: usize = 1000;
    const WRITER_CHANNEL_BUFFER_LENGTH: usize = 1000;

    pub fn new(tcp_stream: net::TcpStream) -> Stream {
        tcp_stream.set_read_timeout(Self::READ_TIMEOUT).expect("Programming error: fail to set Stream read timeout.");
        tcp_stream.set_write_timeout(Self::WRITE_TIMEOUT).expect("Programming error: fail to set Stream write timeout");

        let (to_writer, from_main) = mpsc::sync_channel(Self::WRITER_CHANNEL_BUFFER_LENGTH);
        let (to_main, from_reader) = mpsc::sync_channel(Self::READER_CHANNEL_BUFFER_LENGTH);

        Stream {
            writer: Self::writer(from_main, tcp_stream.try_clone().unwrap()),
            to_writer,
            reader: Self::reader(to_main, tcp_stream.try_clone().unwrap()),
            from_reader: Mutex::new(from_reader),
            peer_addr: tcp_stream.peer_addr().unwrap(),
        }
    }

    pub fn write(&self, msg: &ConsensusMsg) -> Result<(), StreamCorruptedError> {
        self.to_writer.send(msg.clone()).map_err(|_| StreamCorruptedError)
    }

    pub fn read(&self, timeout: Duration) -> Result<ConsensusMsg, StreamReadError> {
        self.from_reader.lock().unwrap().recv_timeout(timeout).map_err(|e| {
            match e {
                RecvTimeoutError::Disconnected => StreamReadError::Corrupted,
                RecvTimeoutError::Timeout => StreamReadError::Timeout,
            }
        })
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}

pub struct StreamCorruptedError;

pub enum StreamReadError {
    Corrupted,
    Timeout,
}

impl Stream {
    // Continuously receives messages from_main and writes into tcp_stream.
    // If it encounters an error other than `ErrorKind::TimedOut` at any point,
    // the thread quietly dies, causing to_writer to become unusable.
    fn writer(
        from_main: mpsc::Receiver<ConsensusMsg>, 
        mut tcp_stream: net::TcpStream,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(msg) = from_main.recv() {
                let bs = msg.serialize();
                if let Err(_) = tcp_stream.write_all(&bs) {
                    break
                };
            }
        })
    }

    // Continuously reads messages from tcp_stream and sends to_main. 
    // If it encounters an error other than `ErrorKind::TimedOut` at any point,
    // the thread quietly dies, causing from_reader to become unusable.
    fn reader(
        to_main: mpsc::SyncSender<ConsensusMsg>,
        tcp_stream: net::TcpStream,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(msg) = ConsensusMsg::deserialize_from_stream(&tcp_stream) {
                if let Err(_) = to_main.send(msg) {
                    break;
                };
            } 
        })
    }
}

trait DeserializeFromStream: Sized {
    fn deserialize_from_stream(tcp_stream: &net::TcpStream) -> Result<Self, DeserializeFromStreamError>;
}

impl DeserializeFromStream for ConsensusMsg {
    fn deserialize_from_stream(mut tcp_stream: &net::TcpStream) -> Result<ConsensusMsg, DeserializeFromStreamError> {
        fn handle_err(err: io::Error) -> DeserializeFromStreamError {
            match err.kind() {
                ErrorKind::TimedOut => DeserializeFromStreamError::TimedOut,
                _ => panic!("Programming error: un-matched ErrorKind while reading from TcpStream.")
            }
        }

        let variant_prefix = {
            let mut buf = [0u8; 1];
            tcp_stream.read_exact(&mut buf).map_err(handle_err)?;
            buf[0]
        };

        let vn = {
            let mut buf = [0u8; 8];
            tcp_stream.read_exact(&mut buf).map_err(handle_err)?;
            ViewNumber::from_le_bytes(buf.try_into().unwrap())
        };

        todo!()
    }
}

impl DeserializeFromStream for QuorumCertificate {
    fn deserialize_from_stream(tcp_stream: &net::TcpStream) -> Result<Self, DeserializeFromStreamError> {
        todo!()
    }
}

impl DeserializeFromStream for Node {
    fn deserialize_from_stream(tcp_stream: &net::TcpStream) -> Result<Self, DeserializeFromStreamError> {
        todo!()
    }
}


enum DeserializeFromStreamError {
    DeserializationError(msg_types::DeserializationError),
    TimedOut,
}