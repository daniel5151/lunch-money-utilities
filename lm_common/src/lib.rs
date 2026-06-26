//! Shared infrastructure for the Lunch Money utility tools.
//!
//! This crate holds the pieces that every tool needs and that were previously
//! copy-pasted across each tool crate: terminal styling, the clap help color
//! scheme, and process bootstrap helpers (TLS provider install + error-chain
//! printer).

pub mod cli;
pub mod config;
pub mod init;
pub mod lm_client;
pub mod style;
pub mod term;
pub mod tool;
