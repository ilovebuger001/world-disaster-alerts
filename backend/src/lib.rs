//! World Disaster Alerts — Rust-only backend.
//!
//! Implements Phase 1 (reliable core) and Phase 2 (realtime and geography) of
//! the Rev.2.9 engineering specification. This is an implementation of the
//! specification's first phases, not a production-qualified release.

pub mod api;
pub mod app;
pub mod config;
pub mod domain;
pub mod error;
pub mod geo;
pub mod http;
pub mod ingest;
pub mod observability;
pub mod realtime;
pub mod reference;
pub mod state;
pub mod storage;
