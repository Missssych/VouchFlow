//! HTTP Middleware
//!
//! Authentication, logging, and tracing middleware

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Trace ID middleware - adds trace_id to requests
pub async fn trace_id_middleware(request: Request<Body>, next: Next) -> Response {
    let trace_id = uuid::Uuid::new_v4().to_string();

    tracing::info!(
        trace_id = %trace_id,
        method = %request.method(),
        uri = %request.uri(),
        "Incoming request"
    );

    let response = next.run(request).await;

    tracing::info!(
        trace_id = %trace_id,
        status = %response.status(),
        "Request completed"
    );

    response
}
