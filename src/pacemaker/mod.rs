//! Subprotocol for synchronizing view numbers.
//!
//! # Introduction
//!
//! ## The need for Byzantine View Synchronization
//!
//! ## Originating work
//!
//! # Deviations from Lewis-Pye
//!
//! ## Minor terminological differences
//!
//! ## `AdvanceView` replaces View Certificates (VCs)
//!
//! # Leader selection

pub mod messages;

pub(crate) mod protocol;

pub mod types;
