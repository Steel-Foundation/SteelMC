//! Point of Interest (POI) type registry.
//!
//! POI types track special blocks (beds, workstations, bells, nether portals, etc.)
//! so game systems can efficiently query for nearby points of interest
//! without scanning every block.

use rustc_hash::FxHashMap;
use steel_utils::{BlockStateId, Identifier};

use crate::blocks::{BlockRef, BlockRegistry};

/// A block whose states belong to a POI type, with optional property constraints.
///
/// An empty `properties` filter matches every state of `block` (vanilla's
/// `getStatesOfBlock`); a non-empty filter keeps only states where each listed property
/// equals the given value (vanilla's `BED_HEADS`-style predicate, used by `home`).
#[derive(Debug)]
pub struct PoiBlockMatcher {
    pub block: BlockRef,
    pub properties: &'static [(&'static str, &'static str)],
}

/// A type of point of interest (e.g., bed, workstation, bell, nether portal).
///
/// Each type matches a set of blocks (optionally filtered by properties) and defines how
/// many entities can claim it via tickets (e.g., a bed has 1 ticket for 1 villager).
#[derive(Debug)]
pub struct PointOfInterestType {
    pub key: Identifier,
    pub blocks: &'static [PoiBlockMatcher],
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
        self.types_by_id.push(poi_type);
        id
    }

    /// Expands every registered POI type's block matchers into the `state -> type`
    /// lookup map. Must be called once after the block registry is fully populated, since
    /// resolving matchers to state ids requires the block registry.
    pub fn build_state_index(&mut self, blocks: &BlockRegistry) {
        self.state_to_type.clear();
        for (id, poi_type) in self.types_by_id.iter().enumerate() {
            for matcher in poi_type.blocks {
                for state_id in blocks.matching_states(matcher.block, matcher.properties) {
                    self.state_to_type.insert(state_id, id);
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use steel_utils::BlockStateId;

    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, vanilla_blocks};

    #[test]
    fn matching_states_respects_property_filter() {
        init_test_registry();
        let blocks = &REGISTRY.blocks;

        let furnace = blocks.matching_states(&vanilla_blocks::BLAST_FURNACE, &[]);
        assert_eq!(
            furnace.len(),
            usize::from(vanilla_blocks::BLAST_FURNACE.state_count())
        );

        let heads = blocks.matching_states(&vanilla_blocks::WHITE_BED, &[("part", "head")]);
        let all_beds = blocks.matching_states(&vanilla_blocks::WHITE_BED, &[]);
        assert_eq!(heads.len() * 2, all_beds.len());
        for state in heads {
            assert!(blocks.get_properties(state).contains(&("part", "head")));
        }
    }

    /// The registry's resolved `state -> type` mapping must exactly equal the state sets
    /// extracted from the vanilla server jar (`build_assets/poi_types.json`) — proving the
    /// block matchers neither under- nor over-match vanilla.
    #[test]
    fn matchers_reproduce_extracted_vanilla_states() {
        init_test_registry();

        #[derive(serde::Deserialize)]
        struct PoiFile {
            poi_types: Vec<PoiJson>,
        }
        #[derive(serde::Deserialize)]
        struct PoiJson {
            name: String,
            block_states: Vec<StateJson>,
        }
        #[derive(serde::Deserialize)]
        struct StateJson {
            state_id: u16,
        }

        let json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/build_assets/poi_types.json"
        ));
        let file: PoiFile = serde_json::from_str(json).unwrap();

        let mut expected: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
        for poi in &file.poi_types {
            let set = expected.entry(poi.name.clone()).or_default();
            set.extend(poi.block_states.iter().map(|s| s.state_id));
        }

        let mut actual: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
        for raw in 0..REGISTRY.blocks.next_state_id {
            if let Some(poi) = REGISTRY.poi_types.type_for_state(BlockStateId(raw)) {
                actual
                    .entry(poi.key.path.to_string())
                    .or_default()
                    .insert(raw);
            }
        }

        assert_eq!(actual, expected);
    }
}
