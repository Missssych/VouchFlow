//! Circuit Breaker pattern for fault tolerance
//! 
//! Prevents cascading failures when provider is unavailable

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,     // Normal operation
    Open,       // Failing, reject requests
    HalfOpen,   // Testing if service recovered
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Duration to stay open before trying half-open
    pub open_duration: Duration,
    /// Number of successes in half-open to close circuit
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration: Duration::from_secs(30),
            success_threshold: 2,
        }
    }
}

/// Circuit breaker for provider calls
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitState>>,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure_time: AtomicU64,
}

impl CircuitBreaker {
    /// Create new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            last_failure_time: AtomicU64::new(0),
        }
    }
    
    /// Check if request is allowed
    pub async fn is_allowed(&self) -> bool {
        let state = *self.state.read().await;
        
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if we should try half-open
                let last_failure = self.last_failure_time.load(Ordering::Relaxed);
                let now = Self::epoch_secs();
                
                if now.saturating_sub(last_failure) >= self.config.open_duration.as_secs() {
                    // Transition to half-open
                    *self.state.write().await = CircuitState::HalfOpen;
                    self.success_count.store(0, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
    
    /// Record a successful operation
    pub async fn record_success(&self) {
        let state = *self.state.read().await;
        
        match state {
            CircuitState::Closed => {
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                let count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count >= self.config.success_threshold {
                    // Close circuit
                    *self.state.write().await = CircuitState::Closed;
                    self.failure_count.store(0, Ordering::Relaxed);
                    tracing::info!("Circuit breaker closed");
                }
            }
            CircuitState::Open => {}
        }
    }
    
    /// Record a failed operation
    pub async fn record_failure(&self) {
        let state = *self.state.read().await;
        
        match state {
            CircuitState::Closed => {
                let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count >= self.config.failure_threshold {
                    // Open circuit
                    *self.state.write().await = CircuitState::Open;
                    self.last_failure_time.store(
                        Self::epoch_secs(),
                        Ordering::Relaxed,
                    );
                    tracing::warn!("Circuit breaker opened after {} failures", count);
                }
            }
            CircuitState::HalfOpen => {
                // Go back to open
                *self.state.write().await = CircuitState::Open;
                self.last_failure_time.store(
                    Self::epoch_secs(),
                    Ordering::Relaxed,
                );
                tracing::warn!("Circuit breaker reopened from half-open");
            }
            CircuitState::Open => {}
        }
    }
    
    /// Get current state
    pub async fn state(&self) -> CircuitState {
        *self.state.read().await
    }

    /// Get current time as seconds since UNIX epoch
    fn epoch_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

impl Clone for CircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            failure_count: AtomicU32::new(self.failure_count.load(Ordering::Relaxed)),
            success_count: AtomicU32::new(self.success_count.load(Ordering::Relaxed)),
            last_failure_time: AtomicU64::new(self.last_failure_time.load(Ordering::Relaxed)),
        }
    }
}
