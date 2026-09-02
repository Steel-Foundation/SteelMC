//! Clientbound filled-map data packet.

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::{
    RegistryReference, data_components::MapId, map_decoration_type::MapDecorationType,
    packets::play::C_MAP_ITEM_DATA,
};
use steel_utils::serial::WriteTo;
use text_components::TextComponent;

/// One icon rendered over a filled map.
#[derive(Clone, Debug, PartialEq)]
pub struct MapDecoration {
    pub decoration_type: RegistryReference<MapDecorationType>,
    pub x: i8,
    pub y: i8,
    pub rotation: i8,
    pub name: Option<TextComponent>,
}

impl MapDecoration {
    #[must_use]
    pub const fn new(
        decoration_type: RegistryReference<MapDecorationType>,
        x: i8,
        y: i8,
        rotation: i8,
        name: Option<TextComponent>,
    ) -> Self {
        Self {
            decoration_type,
            x,
            y,
            rotation: rotation & 15,
            name,
        }
    }
}

impl WriteTo for MapDecoration {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.decoration_type.write(writer)?;
        self.x.write(writer)?;
        self.y.write(writer)?;
        self.rotation.write(writer)?;
        self.name.write(writer)
    }
}

/// Rectangular color update within a 128×128 map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapPatch {
    pub start_x: u8,
    pub start_y: u8,
    pub width: u8,
    pub height: u8,
    pub colors: Vec<u8>,
}

/// Synchronizes filled-map colors and decorations with a client.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_MAP_ITEM_DATA)]
pub struct CMapItemData {
    pub map_id: MapId,
    pub scale: i8,
    pub locked: bool,
    pub decorations: Option<Vec<MapDecoration>>,
    pub color_patch: Option<MapPatch>,
}

impl WriteTo for CMapItemData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.map_id.write(writer)?;
        self.scale.write(writer)?;
        self.locked.write(writer)?;
        self.decorations.write(writer)?;
        let Some(patch) = &self.color_patch else {
            return 0_u8.write(writer);
        };
        patch.width.write(writer)?;
        patch.height.write(writer)?;
        patch.start_x.write(writer)?;
        patch.start_y.write(writer)?;
        patch.colors.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use steel_registry::{
        REGISTRY, Registry, RegistryReference, data_components::MapId, vanilla_map_decoration_types,
    };
    use steel_utils::{codec::VarInt, serial::WriteTo as _};

    use super::{CMapItemData, MapDecoration, MapPatch};

    fn init_test_registry() {
        static INIT_REGISTRY: Once = Once::new();
        INIT_REGISTRY.call_once(|| {
            let mut registry = Registry::new_vanilla();
            registry.freeze();
            let _ = REGISTRY.init(registry);
        });
    }

    #[test]
    fn writes_vanilla_map_packet_shape() {
        init_test_registry();
        let packet = CMapItemData {
            map_id: MapId::new(7),
            scale: 2,
            locked: false,
            decorations: Some(vec![MapDecoration::new(
                RegistryReference::new(&vanilla_map_decoration_types::PLAYER),
                -128,
                127,
                18,
                None,
            )]),
            color_patch: Some(MapPatch {
                start_x: 3,
                start_y: 4,
                width: 2,
                height: 1,
                colors: vec![5, 6],
            }),
        };

        let mut encoded = Vec::new();
        packet
            .write(&mut encoded)
            .expect("map data packet should encode");

        let mut expected = Vec::new();
        VarInt(7)
            .write(&mut expected)
            .expect("map id should encode");
        expected.extend_from_slice(&[2, 0, 1]);
        VarInt(1)
            .write(&mut expected)
            .expect("decoration count should encode");
        VarInt(0)
            .write(&mut expected)
            .expect("player decoration id should encode");
        expected.extend_from_slice(&[128, 127, 2, 0, 2, 1, 3, 4]);
        VarInt(2)
            .write(&mut expected)
            .expect("map color count should encode");
        expected.extend_from_slice(&[5, 6]);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn absent_patch_is_encoded_as_zero_width() {
        init_test_registry();
        let packet = CMapItemData {
            map_id: MapId::new(0),
            scale: 0,
            locked: true,
            decorations: None,
            color_patch: None,
        };

        let mut encoded = Vec::new();
        packet
            .write(&mut encoded)
            .expect("map data packet should encode");
        assert_eq!(encoded, [0, 0, 1, 0, 0]);
    }
}
