//! HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
//! 1. Guaranteed Safety and Liveness in the face of up to 1/3rd of voting power being Byzantine at any given moment,
//! 2. A small API [app::App] for plugging in arbitrary state machine-based applications like blockchains, 
//! 3. Pluggable peer-to-peer [networking] and [persistence],
//! 4. and well-documented, 'obviously correct' source code, designed for easy analysis and testing.

pub mod app;

pub mod config;

pub mod types;

pub mod messages;

pub mod state;

pub mod networking;

pub mod tests;

pub mod validator;