use std::io::{Cursor, Result};

use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::{
    codec::VarInt,
    serial::{ReadFrom, prefixed_read::read_utf},
};

/// Longest handshake hostname accepted, in UTF-16 code units.
pub const MAX_HOST_LENGTH: usize = 255;

#[derive(Clone, Copy, PartialEq, Eq, ReadFrom, Debug)]
#[read(as = VarInt)]
pub enum ClientIntent {
    Status = 1,
    Login = 2,
    Transfer = 3,
}

#[derive(ServerPacket, Clone, Debug)]
pub struct SClientIntention {
    pub protocol_version: i32,
    pub hostname: String,
    pub port: u16,
    pub intention: ClientIntent,
}

impl SClientIntention {
    #[must_use]
    pub const fn new(
        protocol_version: i32,
        hostname: String,
        port: u16,
        intention: ClientIntent,
    ) -> Self {
        Self {
            protocol_version,
            hostname,
            port,
            intention,
        }
    }

    /// Reads the handshake with a hostname limit other than vanilla's [`MAX_HOST_LENGTH`], for
    /// proxies that append forwarding data to the hostname.
    ///
    /// # Errors
    /// Returns an error if the packet is malformed.
    pub fn read_with_host_length(data: &mut Cursor<&[u8]>, max_host_length: usize) -> Result<Self> {
        Ok(Self {
            protocol_version: VarInt::read(data)?.0,
            hostname: read_utf(data, max_host_length)?,
            port: u16::read(data)?,
            intention: ClientIntent::read(data)?,
        })
    }
}

impl ReadFrom for SClientIntention {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::read_with_host_length(data, MAX_HOST_LENGTH)
    }
}
