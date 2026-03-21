use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a full dimension type definition from a data pack JSON file.
#[derive(Debug)]
pub struct DimensionType {
    pub key: Identifier,
    pub fixed_time: Option<i64>,
    pub has_skylight: bool,
    pub has_ceiling: bool,
    pub coordinate_scale: f64,
    pub respawn_anchor_works: bool,
    pub min_y: i32,
    pub height: i32,
    pub logical_height: i32,
    pub infiniburn: &'static str,
    pub ambient_light: f32,
    pub cloud_height: Option<i32>,
    pub has_raids: bool,
    pub monster_spawn_light_level: MonsterSpawnLightLevel,
    pub monster_spawn_block_light_limit: i32,
}

/// Represents the complex structure for monster spawn light level.
#[derive(Debug)]
pub enum MonsterSpawnLightLevel {
    Simple(i32),
    Complex {
        distribution_type: &'static str,
        min_inclusive: i32,
        max_inclusive: i32,
    },
}

pub type DimensionTypeRef = &'static DimensionType;

impl PartialEq for DimensionTypeRef {
    #[expect(clippy::disallowed_methods)] // This IS the PartialEq impl; ptr::eq is correct here
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(*self, *other)
    }
}

impl Eq for DimensionTypeRef {}

pub struct DimensionTypeRegistry {
    dimension_types_by_id: Vec<DimensionTypeRef>,
    dimension_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl DimensionTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            dimension_types_by_id: Vec::new(),
            dimension_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, dimension_type: DimensionTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register dimension types after the registry has been frozen"
        );

        let id = self.dimension_types_by_id.len();
        self.dimension_types_by_key
            .insert(dimension_type.key.clone(), id);
        self.dimension_types_by_id.push(dimension_type);
        id
    }

    /// Replaces a dimension at a given index.
    /// Returns true if the dimension was replaced and false if the dimension wasn't replaced
    #[must_use]
    pub fn replace(&mut self, dimension: DimensionTypeRef, id: usize) -> bool {
        if id >= self.dimension_types_by_id.len() {
            return false;
        }
        self.dimension_types_by_id[id] = dimension;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, DimensionTypeRef)> + '_ {
        self.dimension_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &dt)| (id, dt))
    }

    #[must_use]
    pub fn get_ids(&self) -> Vec<Identifier> {
        self.dimension_types_by_key.keys().cloned().collect()
    }
}

impl Default for DimensionTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    DimensionTypeRegistry,
    DimensionType,
    dimension_types_by_id,
    dimension_types_by_key,
    dimension_types
);
