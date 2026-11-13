//! World Disaster Alerts — Rust backend, release Rev.3.
//!
//! Implements the Rev.2.9 engineering specification's reliable core, realtime
//! and geography phases plus the Rev.3 production work (reference data,
//! security middleware, notification eligibility, metrics). See README.md for
//! what is and is not production-qualified.

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
