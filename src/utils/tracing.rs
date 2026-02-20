//! Tracing and logging utilities

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize tracing/logging subsystem
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,vouchflow=debug"));
    
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .init();
    
    tracing::info!("Tracing initialized");
}

/// Generate a new trace ID
pub fn new_trace_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
