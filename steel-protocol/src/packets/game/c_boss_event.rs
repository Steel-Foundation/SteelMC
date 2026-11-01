//! Boss bar packet, following vanilla `ClientboundBossEventPacket`.

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::play::C_BOSS_EVENT;
use steel_utils::{codec::VarInt, serial::WriteTo};
use text_components::TextComponent;
use uuid::Uuid;

/// Vanilla `BossEvent.BossBarColor` in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossBarColor {
    /// Pink.
    Pink,
    /// Blue.
    Blue,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Purple.
    Purple,
    /// White.
    White,
}

/// Vanilla `BossEvent.BossBarOverlay` in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossBarOverlay {
    /// Continuous bar.
    Progress,
    /// Six segments.
    Notched6,
    /// Ten segments.
    Notched10,
    /// Twelve segments.
    Notched12,
    /// Twenty segments.
    Notched20,
}

/// One boss bar operation.
#[derive(Debug, Clone, PartialEq)]
pub enum BossEventOperation {
    /// Shows a new bar.
    Add {
        /// Title.
        title: TextComponent,
        /// Fill from 0 to 1.
        progress: f32,
        /// Color.
        color: BossBarColor,
        /// Segment style.
        overlay: BossBarOverlay,
        /// Darken screen, play boss music, create fog bits.
        flags: u8,
    },
    /// Hides the bar.
    Remove,
    /// Changes the fill.
    UpdateProgress(f32),
    /// Changes the title.
    UpdateName(TextComponent),
    /// Changes color and segments.
    UpdateStyle(BossBarColor, BossBarOverlay),
    /// Changes the flag bits.
    UpdateProperties(u8),
}

/// Creates, updates or removes a boss bar on one client.
#[derive(ClientPacket, Debug, Clone)]
#[packet_id(Play = C_BOSS_EVENT)]
pub struct CBossEvent {
    /// Bar identity.
    pub id: Uuid,
    /// Operation.
    pub operation: BossEventOperation,
}

impl WriteTo for CBossEvent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.id.write(writer)?;
        match &self.operation {
            BossEventOperation::Add {
                title,
                progress,
                color,
                overlay,
                flags,
            } => {
                VarInt(0).write(writer)?;
                title.write(writer)?;
                progress.write(writer)?;
                VarInt(*color as i32).write(writer)?;
                VarInt(*overlay as i32).write(writer)?;
                flags.write(writer)
            }
            BossEventOperation::Remove => VarInt(1).write(writer),
            BossEventOperation::UpdateProgress(progress) => {
                VarInt(2).write(writer)?;
                progress.write(writer)
            }
            BossEventOperation::UpdateName(title) => {
                VarInt(3).write(writer)?;
                title.write(writer)
            }
            BossEventOperation::UpdateStyle(color, overlay) => {
                VarInt(4).write(writer)?;
                VarInt(*color as i32).write(writer)?;
                VarInt(*overlay as i32).write(writer)
            }
            BossEventOperation::UpdateProperties(flags) => {
                VarInt(5).write(writer)?;
                flags.write(writer)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_use_vanilla_ordinals() {
        let encode = |operation| {
            let mut out = Vec::new();
            CBossEvent {
                id: Uuid::nil(),
                operation,
            }
            .write(&mut out)
            .unwrap_or_else(|error| panic!("boss event should encode: {error}"));
            out[16..].to_vec()
        };
        assert_eq!(encode(BossEventOperation::Remove), [1]);
        assert_eq!(
            encode(BossEventOperation::UpdateProgress(1.0)),
            [2, 0x3F, 0x80, 0, 0]
        );
        assert_eq!(
            encode(BossEventOperation::UpdateStyle(
                BossBarColor::Yellow,
                BossBarOverlay::Notched10
            )),
            [4, 4, 2]
        );
        assert_eq!(encode(BossEventOperation::UpdateProperties(3)), [5, 3]);
    }
}
