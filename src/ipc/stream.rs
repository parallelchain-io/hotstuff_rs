use std::io::{self, ErrorKind, Write, Read};
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::sync::{mpsc, Mutex};
use std::net::{self, SocketAddr};
use std::time::Duration;
use std::mem;
use borsh::BorshSerialize;

use crate::msg_types::{ConsensusMsg, Block, QuorumCertificate, BlockHash, SignatureSet, Signature};

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
    pub fn new(tcp_stream: net::TcpStream, config: StreamConfig) -> Stream {
        tcp_stream.set_read_timeout(Some(config.read_timeout)).expect("Programming error: fail to set Stream read timeout.");
        tcp_stream.set_write_timeout(Some(config.write_timeout)).expect("Programming error: fail to set Stream write timeout");

        let (to_writer, from_main) = mpsc::sync_channel(config.writer_channel_buffer_len);
        let (to_main, from_reader) = mpsc::sync_channel(config.reader_channel_buffer_len);

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
                let bs = msg.try_to_vec().unwrap();
                if let Err(_) = tcp_stream.write_all(&bs) {
                    // This causes the main-to-writer channel to become unusable, marking the Stream as 'corrupt', marking
                    // the stream for reconnection.
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

            // Breaking out of the while loop causes the main-to-reader channel to become unusable, marking the Stream
            // for reconnection.

        })
    }
}

trait DeserializeFromStream: Sized { 
    fn deserialize_from_stream(tcp_stream: &net::TcpStream) -> Result<Self, DeserializeFromStreamError>;
    fn handle_err(err: io::Error) -> DeserializeFromStreamError {
        match err.kind() {
            ErrorKind::TimedOut => DeserializeFromStreamError::TimedOut,
            _ => panic!("Programming error: un-matched ErrorKind while reading from TcpStream.")
        }
    }
}

impl DeserializeFromStream for ConsensusMsg {
    fn deserialize_from_stream(mut tcp_stream: &net::TcpStream) -> Result<ConsensusMsg, DeserializeFromStreamError> {
        todo!()
    }
}

impl DeserializeFromStream for Block {
    fn deserialize_from_stream(mut tcp_stream: &net::TcpStream) -> Result<Self, DeserializeFromStreamError> {
        // Marked as todo pending changes related to turning `command` into `commands`.
        todo!()
    }
}

impl DeserializeFromStream for QuorumCertificate {
    fn deserialize_from_stream(mut tcp_stream: &net::TcpStream) -> Result<Self, DeserializeFromStreamError> {
        let vn = {
            let mut buf = [0u8; mem::size_of::<u64>()];
            tcp_stream.read_exact(&mut buf).map_err(Self::handle_err)?;
            u64::from_le_bytes(buf)
        };

        let block_hash = {
            let mut buf = [0u8; mem::size_of::<BlockHash>()];
            tcp_stream.read_exact(&mut buf).map_err(Self::handle_err)?;
            buf
        };

        let sigs = SignatureSet::deserialize_from_stream(tcp_stream)?;
        
        Ok(QuorumCertificate {
            view_number: vn,
            block_hash,
            sigs
        })
    }
}

impl DeserializeFromStream for SignatureSet {
    fn deserialize_from_stream(mut tcp_stream: &net::TcpStream) -> Result<Self, DeserializeFromStreamError> {
        todo!()
    }
}

pub struct StreamConfig {
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub reader_channel_buffer_len: usize,
    pub writer_channel_buffer_len: usize,
}

enum DeserializeFromStreamError {
    DeserializationError,
    TimedOut,
}