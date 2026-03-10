//! Point of Interest (POI) type registry.
//!
//! POI types track special blocks (beds, workstations, bells, nether portals, etc.)
//! so game systems can efficiently query for nearby points of interest
//! without scanning every block.

use crate::RegistryExt;
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
    pub fn by_id(&self, id: usize) -> Option<PoiTypeRef> {
        self.types_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, poi_type: PoiTypeRef) -> Option<&usize> {
        self.types_by_key.get(&poi_type.key)
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<PoiTypeRef> {
        self.types_by_key.get(key).and_then(|id| self.by_id(*id))
    }

    #[must_use]
    pub fn type_for_state(&self, state_id: BlockStateId) -> Option<PoiTypeRef> {
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

    #[must_use]
    pub fn len(&self) -> usize {
        self.types_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types_by_id.is_empty()
    }

    pub fn register_tag(&mut self, tag: Identifier, poi_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifiers: Vec<Identifier> = poi_keys
            .iter()
            .filter_map(|key| {
                let ident = steel_utils::registry::registry_vanilla_or_custom_tag(key);
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifiers);
    }

    #[must_use]
    pub fn is_in_tag(&self, poi_type: PoiTypeRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|types| types.contains(&poi_type.key))
    }

    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let types = f(existing)
            .into_iter()
            .filter(|key| {
                let exists = self.types_by_key.contains_key(key);
                if !exists {
                    tracing::error!(
                        "POI type {key} not found in registry, skipping from tag {tag}"
                    );
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), types);
    }

    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<PoiTypeRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = PoiTypeRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> + '_ {
        self.tags.keys()
    }
}

impl RegistryExt for PoiTypeRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}
