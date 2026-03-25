use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
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

impl ToNbtTag for &Biome {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList};
        let mut compound = NbtCompound::new();
        compound.insert("has_precipitation", self.has_precipitation);
        compound.insert("temperature", self.temperature);
        compound.insert("downfall", self.downfall);
        compound.insert(
            "temperature_modifier",
            match self.temperature_modifier {
                TemperatureModifier::None => "none",
                TemperatureModifier::Frozen => "frozen",
            },
        );
        compound.insert(
            "creature_spawn_probability",
            self.creature_spawn_probability,
        );

        // Effects
        let mut effects = NbtCompound::new();
        effects.insert("fog_color", self.effects.fog_color);
        effects.insert("sky_color", self.effects.sky_color);
        effects.insert("water_color", self.effects.water_color);
        effects.insert("water_fog_color", self.effects.water_fog_color);
        if let Some(fc) = self.effects.foliage_color {
            effects.insert("foliage_color", fc);
        }
        if let Some(gc) = self.effects.grass_color {
            effects.insert("grass_color", gc);
        }
        if let Some(dfc) = self.effects.dry_foliage_color {
            effects.insert("dry_foliage_color", dfc);
        }
        match self.effects.grass_color_modifier {
            GrassColorModifier::None => {}
            GrassColorModifier::DarkForest => {
                effects.insert("grass_color_modifier", "dark_forest");
            }
            GrassColorModifier::Swamp => {
                effects.insert("grass_color_modifier", "swamp");
            }
        }
        if let Some(ambient_sound) = &self.effects.ambient_sound {
            let s = ambient_sound.to_string();
            effects.insert("ambient_sound", s.as_str());
        }
        if let Some(additions) = &self.effects.additions_sound {
            let mut a = NbtCompound::new();
            let s = additions.sound.to_string();
            a.insert("sound", s.as_str());
            a.insert("tick_chance", additions.tick_chance);
            effects.insert("additions_sound", NbtTag::Compound(a));
        }
        if let Some(mood) = &self.effects.mood_sound {
            let mut m = NbtCompound::new();
            let s = mood.sound.to_string();
            m.insert("sound", s.as_str());
            m.insert("tick_delay", mood.tick_delay);
            m.insert("block_search_extent", mood.block_search_extent);
            m.insert("offset", mood.offset);
            effects.insert("mood_sound", NbtTag::Compound(m));
        }
        if let Some(particle) = &self.effects.particle {
            let mut p = NbtCompound::new();
            let mut opts = NbtCompound::new();
            let s = particle.options.particle_type.to_string();
            opts.insert("type", s.as_str());
            p.insert("options", NbtTag::Compound(opts));
            p.insert("probability", particle.probability);
            effects.insert("particle", NbtTag::Compound(p));
        }
        if let Some(music_list) = &self.effects.music {
            let music_nbt: Vec<NbtCompound> = music_list
                .iter()
                .map(|wm| {
                    let mut wmc = NbtCompound::new();
                    let mut data = NbtCompound::new();
                    data.insert("replace_current_music", wm.data.replace_current_music);
                    data.insert("max_delay", wm.data.max_delay);
                    data.insert("min_delay", wm.data.min_delay);
                    let s = wm.data.sound.to_string();
                    data.insert("sound", s.as_str());
                    wmc.insert("data", NbtTag::Compound(data));
                    wmc.insert("weight", wm.weight);
                    wmc
                })
                .collect();
            effects.insert("music", NbtTag::List(NbtList::Compound(music_nbt)));
        }
        compound.insert("effects", NbtTag::Compound(effects));

        // Spawners
        let mut spawners_compound = NbtCompound::new();
        for (category, entries) in &self.spawners {
            let category_entries: Vec<NbtCompound> = entries
                .iter()
                .map(|sd| {
                    let mut e = NbtCompound::new();
                    let s = sd.entity_type.to_string();
                    e.insert("type", s.as_str());
                    e.insert("weight", sd.weight);
                    e.insert("minCount", sd.min_count);
                    e.insert("maxCount", sd.max_count);
                    e
                })
                .collect();
            spawners_compound.insert(
                category.as_str(),
                NbtTag::List(NbtList::Compound(category_entries)),
            );
        }
        compound.insert("spawners", NbtTag::Compound(spawners_compound));

        // Spawn costs
        let mut spawn_costs_compound = NbtCompound::new();
        for (entity_type, cost) in &self.spawn_costs {
            let mut cost_compound = NbtCompound::new();
            cost_compound.insert("charge", cost.charge);
            cost_compound.insert("energy_budget", cost.energy_budget);
            let s = entity_type.to_string();
            spawn_costs_compound.insert(s.as_str(), NbtTag::Compound(cost_compound));
        }
        compound.insert("spawn_costs", NbtTag::Compound(spawn_costs_compound));

        // Carvers (all treated as "air" step)
        let mut carvers_compound = NbtCompound::new();
        let air_carvers: Vec<String> = self.carvers.iter().map(|id| id.to_string()).collect();
        carvers_compound.insert("air", NbtTag::List(NbtList::from(air_carvers)));
        compound.insert("carvers", NbtTag::Compound(carvers_compound));

        // Features (list of lists)
        let features_nbt: Vec<NbtList> = self
            .features
            .iter()
            .map(|step| {
                let step_strings: Vec<String> = step.iter().map(|id| id.to_string()).collect();
                NbtList::from(step_strings)
            })
            .collect();
        compound.insert("features", NbtTag::List(NbtList::List(features_nbt)));

        NbtTag::Compound(compound)
    }
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
