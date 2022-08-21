use std::thread;
use std::sync::mpsc;
use std::net::{self, SocketAddr};
use std::time::Duration;
use std::io;
use crate::msg_types::ConsensusMsg;

pub struct Stream {
    writer: thread::JoinHandle<()>,
    to_writer: mpsc::Sender<ConsensusMsg>,

    reader: thread::JoinHandle<()>,
    from_reader: mpsc::Receiver<ConsensusMsg>,

    peer_addr: SocketAddr,
}

impl Stream {
    pub fn new(tcp_stream: net::TcpStream) -> Stream {
        let (to_writer, from_main) = mpsc::channel();
        let (to_main, from_reader) = mpsc::channel();

        Stream {
            writer: Self::writer(from_main, tcp_stream.try_clone().unwrap()),
            to_writer,
            reader: Self::reader(to_main, tcp_stream.try_clone().unwrap()),
            from_reader,
            peer_addr: tcp_stream.peer_addr().unwrap(),
        }
    }

    // # Errors
    // ErrorKind::Other: if the channel to the writer thread is broken (read the itemdoc for `Self::writer`).
    pub fn write(&self, msg: &ConsensusMsg) -> io::Result<()> {
        todo!();
        // self.to_writer.send(msg)
    }

    // # Errors
    // 1. ErrorKind::TimedOut.
    // 2. ErrorKind::Other: if the channel to the reader thread is broken (read the itemdoc for `Self::reader`).
    pub fn read(&self, timeout: Duration) -> io::Result<ConsensusMsg> {
        todo!()
        // from_reader.recv_timeout(timeout)
    }

    pub fn peer_addr(&self) -> SocketAddr {
        todo!()
    }
}

impl Stream {
    // Continuously receives messages from_main and writes into tcp_stream.
    // If it encounters an error other than `ErrorKind::TimedOut` at any point,
    // the thread quietly dies, causing to_writer to become unusable.
    fn writer(
        from_main: mpsc::Receiver<ConsensusMsg>, 
        tcp_stream: net::TcpStream,
    ) -> thread::JoinHandle<()> {
        todo!()
    }

    // Continuously reads messages from tcp_stream and sends to_main. 
    // If it encounters an error other than `ErrorKind::TimedOut` at any point,
    // the thread quietly dies, causing from_reader to become unusable.
    fn reader(
        to_main: mpsc::Sender<ConsensusMsg>,
        tcp_stream: net::TcpStream,
    ) -> thread::JoinHandle<()> {
        todo!()
    }
}
