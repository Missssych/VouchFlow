//! Axum HTTP Server
//!
//! API Gateway for receiving transaction requests

use axum::{Router, routing::get};
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::handlers;
use crate::infrastructure::channels::CommandSender;
use crate::infrastructure::database::Database;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub command_tx: CommandSender,
    pub db: Database,
}

/// API Gateway server
pub struct Gateway {
    state: AppState,
}

impl Gateway {
    /// Create new gateway
    pub fn new(command_tx: CommandSender, db: Database) -> Self {
        Self {
            state: AppState { command_tx, db },
        }
    }

    /// Create router with all routes
    fn create_router(&self) -> Router {
        // CORS layer for development
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            // Main transaction endpoint
            .route("/api/v1/transaksi", get(handlers::handle_transaksi))
            // Health check
            .route("/health", get(handlers::health_check))
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .with_state(self.state.clone())
    }

    /// Start serving HTTP requests
    pub async fn serve(self, addr: &str) -> Result<(), std::io::Error> {
        let router = self.create_router();
        let addr: SocketAddr = addr.parse().expect("Invalid address");

        tracing::info!("API Gateway listening on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router).await?;

        Ok(())
    }
}
