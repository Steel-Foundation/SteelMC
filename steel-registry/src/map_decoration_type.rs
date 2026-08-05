//! Map decoration type registry values extracted from Vanilla.

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Registered visual and tracking behavior for a map decoration.
#[derive(Debug)]
pub struct MapDecorationType {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub show_on_item_frame: bool,
    pub map_color: i32,
    pub exploration_map_element: bool,
    pub track_count: bool,
}

impl MapDecorationType {
    pub const NO_MAP_COLOR: i32 = -1;

    #[must_use]
    pub const fn new(
        key: Identifier,
        asset_id: Identifier,
        show_on_item_frame: bool,
        map_color: i32,
        exploration_map_element: bool,
        track_count: bool,
    ) -> Self {
        Self {
            key,
            asset_id,
            show_on_item_frame,
            map_color,
            exploration_map_element,
            track_count,
        }
    }

    #[must_use]
    pub const fn has_map_color(&self) -> bool {
        self.map_color != Self::NO_MAP_COLOR
    }
}

pub type MapDecorationTypeRef = &'static MapDecorationType;

pub struct MapDecorationTypeRegistry {
    map_decoration_types_by_id: Vec<MapDecorationTypeRef>,
    map_decoration_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl MapDecorationTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            map_decoration_types_by_id: Vec::new(),
            map_decoration_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    MapDecorationTypeRegistry,
    MapDecorationTypeRef,
    map_decoration_types_by_id,
    map_decoration_types_by_key,
    allows_registering
);

crate::impl_registry!(
    MapDecorationTypeRegistry,
    MapDecorationType,
    map_decoration_types_by_id,
    map_decoration_types_by_key,
    map_decoration_types
);

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;

    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    #[test]
    fn extracted_types_follow_vanilla_registry_order_and_fields() {
        init_test_registry();
        assert_eq!(REGISTRY.map_decoration_types.len(), 35);

        let player = REGISTRY
            .map_decoration_types
            .by_id(0)
            .expect("player decoration should be registered first");
        assert_eq!(player.key, Identifier::vanilla_static("player"));
        assert_eq!(player.asset_id, Identifier::vanilla_static("player"));
        assert!(!player.show_on_item_frame);
        assert!(!player.has_map_color());
        assert!(!player.exploration_map_element);
        assert!(player.track_count);

        let trial_chambers = REGISTRY
            .map_decoration_types
            .by_id(34)
            .expect("trial chambers decoration should be registered last");
        assert_eq!(
            trial_chambers.key,
            Identifier::vanilla_static("trial_chambers")
        );
        assert_eq!(trial_chambers.map_color, 12_741_452);
        assert!(trial_chambers.exploration_map_element);
        assert!(!trial_chambers.track_count);
    }
}
