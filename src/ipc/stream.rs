use std::collections::VecDeque;
use std::io::{Write, Read};
use std::sync::mpsc::RecvTimeoutError;
use std::{thread, mem};
use std::sync::{mpsc, Mutex};
use std::net::{self, SocketAddr};
use std::time::Duration;
use borsh::{BorshSerialize, BorshDeserialize};
use crate::msg_types::ConsensusMsg;

/// Stream is a wrapper around TcpStream which implements in-the-background reads and writes of ConsensusMsgs.
pub enum Stream {
    Foreign {
        to_writer: mpsc::SyncSender<ConsensusMsg>,
    
        // Wrapped inside a Mutex so that Stream can be Sync. If Stream ends up never being shared between threads, the overhead of locking
        // is cheap enough that we deem it acceptable. 
        from_reader: Mutex<mpsc::Receiver<ConsensusMsg>>,
    
        peer_addr: SocketAddr,
    },
    Loopback(Mutex<VecDeque<ConsensusMsg>>),
}

impl Stream {
    pub fn new(tcp_stream: net::TcpStream, config: StreamConfig) -> Stream {
        tcp_stream.set_read_timeout(Some(config.read_timeout)).expect("Programming error: fail to set Stream read timeout.");
        tcp_stream.set_write_timeout(Some(config.write_timeout)).expect("Programming error: fail to set Stream write timeout");

        let (to_writer, from_main) = mpsc::sync_channel(config.writer_channel_buffer_len);
        let (to_main, from_reader) = mpsc::sync_channel(config.reader_channel_buffer_len);

        // Start reader and writer threads.
        Self::writer(from_main, tcp_stream.try_clone().unwrap());
        Self::reader(to_main, tcp_stream.try_clone().unwrap());

        Stream::Foreign {
            to_writer,
            from_reader: Mutex::new(from_reader),
            peer_addr: tcp_stream.peer_addr().unwrap(),
        }
    }

    pub fn new_loopback(config: StreamConfig) -> Stream {
        Stream::Loopback(Mutex::new(VecDeque::with_capacity(config.reader_channel_buffer_len)))
    }

    pub fn write(&self, msg: &ConsensusMsg) -> Result<(), StreamCorruptedError> {
        match self {
            Self::Foreign { to_writer, .. } => to_writer.send(msg.clone()).map_err(|_| StreamCorruptedError)?,
            Self::Loopback(deque) => deque.lock().unwrap().push_back(msg.clone()),
        };

        Ok(())
    }

    pub fn read(&self, timeout: Duration) -> Result<ConsensusMsg, StreamReadError> {
        match self {
            Self::Foreign { from_reader, .. } => from_reader.lock().unwrap().recv_timeout(timeout).map_err(|e| match e {
                RecvTimeoutError::Disconnected => StreamReadError::Corrupted,
                RecvTimeoutError::Timeout => StreamReadError::Timeout,
            }),
            Self::Loopback(deque) => match deque.lock().unwrap().pop_front() {
                Some(msg) => Ok(msg),
                None => Err(StreamReadError::LoopbackEmpty)
            },
        }
    }

    // # Panics
    pub fn peer_addr(&self) -> Result<SocketAddr, IsLoopbackError> {
        match self {
            Self::Foreign { peer_addr, .. } => Ok(*peer_addr),
            Self::Loopback(_) => Err(IsLoopbackError),
        }
    }
}

pub struct StreamCorruptedError;

pub enum StreamReadError {
    Corrupted,
    Timeout,
    LoopbackEmpty,
}

#[derive(Debug)]
pub struct IsLoopbackError;

impl Stream {
    // Continuously receives messages from_main and writes into tcp_stream.
    // If it encounters an error other than `ErrorKind::TimedOut` at any point, the thread quietly dies, causing to_writer to become 
    // unusable. Calls to Stream::write will fail following this. It is then the caller's responsibility to discard the Stream and 
    // re-establish it, if necessary.
    fn writer(
        from_main: mpsc::Receiver<ConsensusMsg>, 
        mut tcp_stream: net::TcpStream,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            // Read messages from Main.
            while let Ok(msg) = from_main.recv() {

                // Serialize message.
                let bs = msg.try_to_vec().unwrap();
                let bs_len = (bs.len() as u64).to_le_bytes();

                // Write length of serialized message.
                if let Err(_) = tcp_stream.write_all(&bs_len) {
                    // If a write operation fails, break out of the while loop, ending the Writer thread.
                    break
                }
                // Write serialized message.
                if let Err(_) = tcp_stream.write_all(&bs) { 
                    break
                };
            }
        })
    }

    // Continuously reads messages from tcp_stream and sends to_main. 
    // If it encounters an error other than `ErrorKind::TimedOut` at any point, the thread quietly dies, causing from_reader to become 
    // unusable. Calls to Stream::read will fail following this. It is then the caller's responsibility to discard the Stream and re-
    // establish it, if necessary.
    fn reader(
        to_main: mpsc::SyncSender<ConsensusMsg>,
        mut tcp_stream: net::TcpStream,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            loop {
                // Read length of forthcoming serialized message.
                let bs_len = {
                    let mut buf = [0u8; mem::size_of::<u64>()];
                    if let Err(_) = tcp_stream.read_exact(&mut buf) {
                        // If a read operation fails, break out of the while loop, ending the Reader thread.
                        break
                    }
                    usize::from_le_bytes(buf.try_into().unwrap())
                };
                
                // Read message.
                let bs = {
                    let mut buf = vec![0u8; bs_len];
                    if let Err(_) = tcp_stream.read_exact(&mut buf) {
                        break
                    }
                    buf
                };

                // Deserialize message.
                let msg = match ConsensusMsg::deserialize(&mut bs.as_slice()) {
                    Ok(msg) => msg,
                    // If a recevied message cannot be deserialized, break out of the while loop, ending the Reader thread.
                    Err(_) => break
                };

                // Send read message to Main.
                if let Err(_) = to_main.send(msg) {
                    // If connection to main is severed, this means that this Stream has been dropped. This Stream will never be read from again,
                    // so we drop it.
                    break
                }
            }
        })
    }
}

pub struct StreamConfig {
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub reader_channel_buffer_len: usize,
    pub writer_channel_buffer_len: usize,
}
