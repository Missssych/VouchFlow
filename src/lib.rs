//! App-Voucher - Voucher Transaction Application
//!
//! Aplikasi desktop untuk transaksi voucher dengan arsitektur:
//! - Event-Driven: komunikasi via channel
//! - Single-Writer SQLite: serialisasi write
//! - Central Store (Read-Model): UI baca dari memory
//! - Gated UI Rendering: render hanya menu aktif

pub mod application;
pub mod config;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod utils;
