//! Steel Login - Handles authentication, login protocol, and connection lifecycle.
//!
//! This crate manages:
//! - Pre-play TCP client connection (`JavaTcpClient`)
//! - Mojang authentication
//! - Login, configuration, and status state handlers
//! - Type re-exports for convenience

mod authentication;
mod connection;
mod handlers;
mod login;
mod tcp_client;

// Authentication
pub use authentication::{AuthError, TextureError, mojang_authenticate, signed_bytes_be_to_hex};

// Login helpers
pub use login::{is_valid_player_name, offline_uuid};

// Type re-exports from steel-core
pub use steel_core::player::{ClientInformation, GameProfile, GameProfileAction};

// Connection types
pub use connection::JavaConnection;
pub use tcp_client::{ConnectionUpdate, JavaTcpClient};
