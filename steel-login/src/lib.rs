//! Steel Login - Handles authentication, login protocol, and connection lifecycle.
//!
//! This crate manages:
//! - Pre-play TCP client connection (`JavaTcpClient`)
//! - Mojang authentication
//! - Login, configuration, and status state handlers
//! - Type re-exports for convenience

// Auto-trait (`Send`/`Sync`) proofs through the World/chunk type graph exceed the default depth of 128.
#![recursion_limit = "256"]

mod authentication;
mod connection;
mod handlers;
mod pre_play_state;
mod tcp_client;

// Authentication
pub use authentication::{AuthError, TextureError, mojang_authenticate, signed_bytes_be_to_hex};

// Type re-exports from steel-core
pub use steel_core::player::{ClientInformation, GameProfile, is_valid_player_name, offline_uuid};

// Connection types
pub use connection::JavaConnection;
pub use tcp_client::{ConnectionUpdate, JavaTcpClient, ServerConnectionSession};
