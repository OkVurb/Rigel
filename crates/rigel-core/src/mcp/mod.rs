//! MCP helpers owned by the core (roadmap 69).
//!
//! The transport rules live here rather than in a connector so every caller applies the
//! same checks: a scrubbed stdio environment, no unauthenticated HTTP, no private-network
//! URL, and a bounded message.

pub mod transport;

pub use transport::{
    ENVIRONMENT_ALLOWLIST, MAX_MESSAGE_BYTES, TransportError, check_http, check_message,
    check_stdio_environment, scrub_environment,
};
