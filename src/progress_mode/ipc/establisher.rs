use std::thread;
use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::net::{SocketAddr, IpAddr, TcpStream, TcpListener};
use std::time::Duration;
use std::io::ErrorKind;
use rand::{self, Rng};
use indexmap::{IndexSet, IndexMap};
use crate::msg_types::PublicAddr;
use crate::progress_mode::ipc;

pub struct Establisher {
    initiator: thread::JoinHandle<()>,
    to_initiator: mpsc::Sender<EstablisherCmd>,
    listener: thread::JoinHandle<()>,
    to_listener: mpsc::Sender<EstablisherCmd>,
    config: EstablisherConfig,
}

impl Establisher {
    pub fn new(config: EstablisherConfig) -> (Establisher, mpsc::Receiver<EstablisherResult>) {
        let (to_main, from_establishers) = mpsc::channel();
        let (to_initiator, initiator_from_main) = mpsc::channel();
        let (to_listener, listener_from_main) = mpsc::channel();

        let establisher = Establisher {
            initiator: Self::start_initiator(config.clone(), initiator_from_main, to_main.clone()),
            to_initiator,
            listener: Self::start_listener(config.clone(), listener_from_main, to_main),
            to_listener,
            config
        };

        (establisher, from_establishers)
    }

    pub fn connect_later(&self, target: (PublicAddr, IpAddr)) {
        match target.0 {
            target_public_addr if target_public_addr > self.config.my_public_addr => self.to_initiator.send(EstablisherCmd::Connect(target)).expect("Programming error: connection between main thread and Initiator thread lost."),
            target_public_addr if target_public_addr <= self.config.my_public_addr => self.to_listener.send(EstablisherCmd::Connect(target)).expect("Programming error: connection between main thread and Listener thread lost."),
            _ => unreachable!(),
        };
    }

    pub fn cancel_later(&self, target: (PublicAddr, IpAddr)) {
        match target.0 {
            target_public_addr if target_public_addr > self.config.my_public_addr => self.to_initiator.send(EstablisherCmd::Cancel(target)).expect("Programming error: connection between main thread and Initiator thread lost."),
            target_public_addr if target_public_addr <= self.config.my_public_addr => self.to_listener.send(EstablisherCmd::Cancel(target)).expect("Programming error: connection between main thread and Listener thread lost."),
            _ => unreachable!(),
        };
    }

    fn start_initiator(
        config: EstablisherConfig,
        from_main: mpsc::Receiver<EstablisherCmd>,
        to_main: mpsc::Sender<EstablisherResult>
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut pending_targets = IndexSet::new();
            loop {
                // 1. Receive commands from main, if any, and update tasks list.
                if Self::update_pending_targets_initiator(&mut pending_targets, &from_main) == TryRecvError::Disconnected {
                    return
                }

                // 2. Pick random target from pending targets. 
                let random_idx = rng.gen_range(0..pending_targets.len());
                let target = pending_targets.get_index(random_idx).unwrap();
                let target_socket_addr = SocketAddr::new(target.1, config.listening_port);

                // 3. Attempt to establish stream to pending target.
                let stream = match TcpStream::connect_timeout(&target_socket_addr, config.initiator_timeout) {
                    Ok(stream) => stream,
                    Err(e) => match e.kind() {
                        ErrorKind::TimedOut => continue,
                        _ => panic!("Programming error: unmatched io::ErrorKind."),
                    }
                };

                // 4. Send established connection to main.
                to_main.send(EstablisherResult((target.0, Arc::new(ipc::Stream::new(stream)))));
            }
        })
    }

    fn start_listener(
        config: EstablisherConfig,
        from_main: mpsc::Receiver<EstablisherCmd>,
        to_main: mpsc::Sender<EstablisherResult>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut pending_targets = IndexMap::new();
            let listening_socket_addr = SocketAddr::new(config.listening_addr, config.listening_port);
            let listener = TcpListener::bind(listening_socket_addr).expect("Programming or Configuration error: unable to bind TcpListener");

            // 1. Bind TcpListener to listening port.
            for stream in listener.incoming() {
                // 2. Accept incoming connections
                let stream = stream.expect("Programming error: unmatched ErrorKind when accepting incoming stream.");

                // 3. Receive commands from main, if any, and update tasks list.
                if Self::update_pending_targets_listener(&mut pending_targets, &from_main) == TryRecvError::Disconnected {
                    return
                }

                // 4. If established connection is in tasks list, send to main. 
                if let Some(public_addr) =  pending_targets.get(&stream.peer_addr().unwrap().ip()) {
                    to_main.send(EstablisherResult((*public_addr, Arc::new(ipc::Stream::new(stream)))));
                }
            }
        })
    }

    fn update_pending_targets_initiator(pending_targets: &mut IndexSet<(PublicAddr, IpAddr)>, from_main: &mpsc::Receiver<EstablisherCmd>) -> TryRecvError {
        loop {
            match from_main.try_recv() {
                Ok(cmd) => match cmd {
                    EstablisherCmd::Connect(target) => pending_targets.insert(target),
                    EstablisherCmd::Cancel(target) => pending_targets.remove(&target),
                },
                Err(e) => return e,
            };
        }
    }

    fn update_pending_targets_listener(pending_targets: &mut IndexMap<IpAddr, PublicAddr>, from_main: &mpsc::Receiver<EstablisherCmd>) -> TryRecvError {
        loop {
            match from_main.try_recv() {
                Ok(cmd) => match cmd {
                    EstablisherCmd::Connect(target) => pending_targets.insert(target.1, target.0),
                    EstablisherCmd::Cancel(target) => pending_targets.remove(&target.1),
                },
                Err(e) => return e,
            };
        }

    }
}


#[derive(Clone)]
pub struct EstablisherConfig {
    pub listening_addr: IpAddr,
    pub listening_port: u16,
    pub my_public_addr: PublicAddr,
    pub initiator_timeout: Duration, 
}

enum EstablisherCmd {
    Connect((PublicAddr, IpAddr)),
    Cancel((PublicAddr, IpAddr)),
}

pub struct EstablisherResult(pub (PublicAddr, Arc<ipc::Stream>));
