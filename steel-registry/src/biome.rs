use rustc_hash::FxHashMap;
use steel_utils::Identifier;

#[derive(Debug)]
pub struct Biome {
    pub key: Identifier,
    pub has_precipitation: bool,
    pub temperature: f32,
    pub downfall: f32,
    pub temperature_modifier: TemperatureModifier,
    pub effects: BiomeEffects,
    pub creature_spawn_probability: f32,
    pub spawners: FxHashMap<String, Vec<SpawnerData>>,
    pub spawn_costs: FxHashMap<Identifier, SpawnCost>,
    pub carvers: Vec<Identifier>,
    pub features: Vec<Vec<Identifier>>,
}

#[derive(Debug)]
pub struct BiomeEffects {
    pub fog_color: i32,
    pub sky_color: i32,
    pub water_color: i32,
    pub water_fog_color: i32,
    pub foliage_color: Option<i32>,
    pub grass_color: Option<i32>,
    pub dry_foliage_color: Option<i32>,
    pub grass_color_modifier: GrassColorModifier,
    pub music: Option<Vec<WeightedMusic>>,
    pub ambient_sound: Option<Identifier>,
    pub additions_sound: Option<AdditionsSound>,
    pub mood_sound: Option<MoodSound>,
    pub particle: Option<Particle>,
}

#[derive(Debug)]
pub struct SpawnerData {
    pub entity_type: Identifier,
    pub weight: i32,
    pub min_count: i32,
    pub max_count: i32,
}

#[derive(Debug)]
pub struct SpawnCost {
    pub energy_budget: f64,
    pub charge: f64,
}

#[derive(Debug, Default)]
pub enum TemperatureModifier {
    #[default]
    None,
    Frozen,
}

#[derive(Debug)]
pub enum GrassColorModifier {
    None,
    DarkForest,
    Swamp,
}

#[derive(Debug)]
pub struct WeightedMusic {
    pub data: Music,
    pub weight: i32,
}

#[derive(Debug)]
pub struct Music {
    pub replace_current_music: bool,
    pub max_delay: i32,
    pub min_delay: i32,
    pub sound: Identifier,
}

#[derive(Debug)]
pub struct AdditionsSound {
    pub sound: Identifier,
    pub tick_chance: f64,
}

#[derive(Debug)]
pub struct MoodSound {
    pub sound: Identifier,
    pub tick_delay: i32,
    pub block_search_extent: i32,
    pub offset: f64,
}

#[derive(Debug)]
pub struct Particle {
    pub options: ParticleOptions,
    pub probability: f32,
}

#[derive(Debug)]
pub struct ParticleOptions {
    pub particle_type: Identifier,
}

pub type BiomeRef = &'static Biome;

pub struct BiomeRegistry {
    biomes_by_id: Vec<BiomeRef>,
    biomes_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl BiomeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            biomes_by_id: Vec::new(),
            biomes_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, biome: BiomeRef, key: Identifier) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register biomes after the registry has been frozen"
        );

        let id = self.biomes_by_id.len();
        self.biomes_by_key.insert(key, id);
        self.biomes_by_id.push(biome);
        id
    }

    /// Replaces a biome at a given index.
    /// Returns true if the biome was replaced and false if the biome wasn't replaced
    #[must_use]
    pub fn replace(&mut self, biome: BiomeRef, id: usize) -> bool {
        if id >= self.biomes_by_id.len() {
            return false;
        }
        self.biomes_by_id[id] = biome;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, BiomeRef)> + '_ {
        self.biomes_by_id
            .iter()
            .enumerate()
            .map(|(id, &biome)| (id, biome))
    }
}

impl Default for BiomeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(BiomeRegistry, Biome, biomes_by_id, biomes_by_key, biomes);
