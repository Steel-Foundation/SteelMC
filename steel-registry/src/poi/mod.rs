//! Point of Interest (POI) type registry.
//!
//! POI types track special blocks (beds, workstations, bells, nether portals, etc.)
//! so game systems can efficiently query for nearby points of interest
//! without scanning every block.

use rustc_hash::FxHashMap;
use steel_utils::{BlockStateId, Identifier};

/// A type of point of interest (e.g., bed, workstation, bell, nether portal).
///
/// Each type maps to specific block states and defines how many entities
/// can claim it via tickets (e.g., a bed has 1 ticket for 1 villager).
#[derive(Debug, Clone)]
pub struct PointOfInterestType {
    pub key: Identifier,
    pub block_state_ids: &'static [BlockStateId],
    pub ticket_count: u32,
    pub search_distance: u32,
}

/// Static reference to a POI type definition.
pub type PoiTypeRef = &'static PointOfInterestType;

/// Registry of all POI types, with reverse lookup from block state to type.
pub struct PoiTypeRegistry {
    types_by_id: Vec<PoiTypeRef>,
    types_by_key: FxHashMap<Identifier, usize>,
    /// O(1) block state -> POI type ID lookup.
    state_to_type: FxHashMap<BlockStateId, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl Default for PoiTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PoiTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types_by_id: Vec::new(),
            types_by_key: FxHashMap::default(),
            state_to_type: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, poi_type: PoiTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register POI types after the registry has been frozen"
        );

        let id = self.types_by_id.len();
        self.types_by_key.insert(poi_type.key.clone(), id);

        for &state_id in poi_type.block_state_ids {
            self.state_to_type.insert(state_id, id);
        }

        self.types_by_id.push(poi_type);
        id
    }

    #[must_use]
    pub fn type_for_state(&self, state_id: BlockStateId) -> Option<PoiTypeRef> {
        use crate::RegistryExt;
        self.state_to_type
            .get(&state_id)
            .and_then(|id| self.by_id(*id))
    }

    #[must_use]
    pub fn type_id_for_state(&self, state_id: BlockStateId) -> Option<usize> {
        self.state_to_type.get(&state_id).copied()
    }

    #[must_use]
    pub fn is_poi_state(&self, state_id: BlockStateId) -> bool {
        self.state_to_type.contains_key(&state_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, PoiTypeRef)> + '_ {
        self.types_by_id
            .iter()
            .enumerate()
            .map(|(id, &poi_type)| (id, poi_type))
    }
}

crate::impl_registry!(
    PoiTypeRegistry,
    PointOfInterestType,
    types_by_id,
    types_by_key,
    poi_types
);
crate::impl_tagged_registry!(PoiTypeRegistry, types_by_key, "POI type");
