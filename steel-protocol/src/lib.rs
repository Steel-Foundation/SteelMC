//! # Steel Protocol
//!
//! The core library for the Steel Minecraft server. Handles everything related to the PLAY state.
#![expect(
    clippy::absolute_paths,
    clippy::allow_attributes_without_reason,
    clippy::match_same_arms,
    clippy::ref_option,
    clippy::unreadable_literal,
    reason = "packet definitions keep explicit protocol-oriented code and existing test/build invariants"
)]
#![cfg_attr(
    test,
    expect(
        clippy::float_cmp,
        clippy::unwrap_used,
        reason = "packet round-trip tests compare exact serialized float values and unwrap known-good buffers"
    )
)]

pub mod packet_reader;
pub mod packet_traits;
pub mod packet_writer;
pub mod packets;
pub mod utils;
