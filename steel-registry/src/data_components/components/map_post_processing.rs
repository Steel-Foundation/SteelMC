//! Vanilla `minecraft:map_post_processing` transient item component.

use std::io::{Cursor, Result, Write};

use steel_utils::codec::VarInt;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Operation applied to a filled map after crafting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapPostProcessing {
    Lock,
    Scale,
}

impl MapPostProcessing {
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Lock => 0,
            Self::Scale => 1,
        }
    }

    const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Scale,
            _ => Self::Lock,
        }
    }
}

impl WriteTo for MapPostProcessing {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.id()).write(writer)
    }
}

impl ReadFrom for MapPostProcessing {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::from_id(VarInt::read(data)?.0))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::MapPostProcessing;

    #[test]
    fn network_ids_match_vanilla() {
        for (value, id) in [(MapPostProcessing::Lock, 0), (MapPostProcessing::Scale, 1)] {
            let mut encoded = Vec::new();
            value.write(&mut encoded).expect("value should encode");
            assert_eq!(
                VarInt::read(&mut Cursor::new(encoded.as_slice()))
                    .expect("encoded ID should decode")
                    .0,
                id
            );
            assert_eq!(
                MapPostProcessing::read(&mut Cursor::new(encoded.as_slice()))
                    .expect("map post-processing value should decode"),
                value
            );
        }
    }

    #[test]
    fn out_of_bounds_network_ids_fall_back_to_lock() {
        for id in [-1, 2, i32::MAX] {
            let mut encoded = Vec::new();
            VarInt(id).write(&mut encoded).expect("id should encode");
            assert_eq!(
                MapPostProcessing::read(&mut Cursor::new(encoded.as_slice()))
                    .expect("out-of-bounds ID should decode"),
                MapPostProcessing::Lock
            );
        }
    }
}
