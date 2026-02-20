//! Configuration management for the application

use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Database file path
    pub db_path: PathBuf,
    /// API server host
    pub server_host: String,
    /// API server port
    pub server_port: u16,
    /// Terminal host (for provider communication)
    pub terminal_host: String,
    /// Terminal port
    pub terminal_port: u16,
    /// Command bus capacity
    pub command_bus_capacity: usize,
    /// DB command queue capacity
    pub db_command_capacity: usize,
    /// Event bus capacity
    pub event_bus_capacity: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("voucher.db"),
            server_host: "127.0.0.1".to_string(),
            server_port: 8080,
            terminal_host: "127.0.0.1".to_string(),
            terminal_port: 8081,
            command_bus_capacity: 100,
            db_command_capacity: 100,
            event_bus_capacity: 1000,
        }
    }
}

impl AppConfig {
    /// Create config from environment or defaults
    pub fn from_env() -> Self {
        Self::default()
    }
    
    /// Get full server address
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server_host, self.server_port)
    }
    
    /// Get full terminal address
    pub fn terminal_addr(&self) -> String {
        format!("{}:{}", self.terminal_host, self.terminal_port)
    }
}
