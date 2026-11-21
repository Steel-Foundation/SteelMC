use std::io::{self, Write};

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{
    RegistryReference, data_components::vanilla_components::MapId,
    map_decoration_type::MapDecorationType, packets::play::C_MAP_ITEM_DATA,
};
use steel_utils::{codec::VarInt, serial::WriteTo};
use text_components::TextComponent;

const MAP_SIZE: usize = 128;

/// One visible marker in a map update.
#[derive(Clone, Debug, PartialEq, Eq, WriteTo)]
pub struct MapDecoration {
    /// Registry-backed marker appearance.
    pub decoration_type: RegistryReference<MapDecorationType>,
    /// Horizontal half-pixel coordinate relative to the map center.
    pub x: i8,
    /// Vertical half-pixel coordinate relative to the map center.
    pub y: i8,
    /// Rotation in sixteenths of a turn.
    pub rotation: u8,
    /// Optional displayed name.
    pub name: Option<TextComponent>,
}

/// A rectangular update in row-major pixel order.
#[derive(Clone, Debug)]
pub struct MapPatch {
    /// First column.
    pub start_x: u8,
    /// First row.
    pub start_y: u8,
    /// Number of columns.
    pub width: u8,
    /// Number of rows.
    pub height: u8,
    /// Packed map colors, indexed by column + row * width.
    pub colors: Vec<u8>,
}

/// Updates map metadata, decorations, and changed pixels independently.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_MAP_ITEM_DATA)]
pub struct CMapItemData {
    /// Domain-local saved map identity.
    pub map_id: MapId,
    /// Map zoom level.
    pub scale: i8,
    /// Whether terrain updates are disabled.
    pub locked: bool,
    /// `None` preserves existing client decorations; empty clears them.
    pub decorations: Option<Vec<MapDecoration>>,
    /// `None` preserves existing pixels.
    pub color_patch: Option<MapPatch>,
}

impl WriteTo for CMapItemData {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.map_id.write(writer)?;
        self.scale.write(writer)?;
        self.locked.write(writer)?;
        self.decorations.write(writer)?;
        let Some(patch) = &self.color_patch else {
            return 0_u8.write(writer);
        };
        let expected = usize::from(patch.width) * usize::from(patch.height);
        if patch.width == 0
            || patch.height == 0
            || usize::from(patch.start_x) + usize::from(patch.width) > MAP_SIZE
            || usize::from(patch.start_y) + usize::from(patch.height) > MAP_SIZE
            || patch.colors.len() != expected
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid map patch dimensions",
            ));
        }
        patch.width.write(writer)?;
        patch.height.write(writer)?;
        patch.start_x.write(writer)?;
        patch.start_y.write(writer)?;
        VarInt(expected as i32).write(writer)?;
        writer.write_all(&patch.colors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_decorations_preserve_markers_while_empty_decorations_clear_them() {
        let mut packet = CMapItemData {
            map_id: MapId::new(300),
            scale: 2,
            locked: true,
            decorations: None,
            color_patch: None,
        };
        let mut bytes = Vec::new();
        packet.write(&mut bytes).expect("map update");
        assert_eq!(bytes, [0xac, 0x02, 2, 1, 0, 0]);
        packet.decorations = Some(Vec::new());
        bytes.clear();
        packet.write(&mut bytes).expect("clear markers");
        assert_eq!(bytes, [0xac, 0x02, 2, 1, 1, 0, 0]);
    }

    #[test]
    fn patch_writes_dimensions_offsets_and_row_major_bytes_and_rejects_invalid_bounds() {
        let mut packet = CMapItemData {
            map_id: MapId::new(0),
            scale: 0,
            locked: false,
            decorations: None,
            color_patch: Some(MapPatch {
                start_x: 126,
                start_y: 127,
                width: 2,
                height: 1,
                colors: vec![17, 42],
            }),
        };
        let mut bytes = Vec::new();
        packet.write(&mut bytes).expect("edge patch");
        assert_eq!(bytes, [0, 0, 0, 0, 2, 1, 126, 127, 2, 17, 42]);
        packet.color_patch.as_mut().expect("patch").start_x = 127;
        assert_eq!(
            packet
                .write(&mut Vec::new())
                .expect_err("out of bounds")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let patch = packet.color_patch.as_mut().expect("patch");
        patch.start_x = 126;
        patch.colors.pop();
        assert_eq!(
            packet
                .write(&mut Vec::new())
                .expect_err("short pixels")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
