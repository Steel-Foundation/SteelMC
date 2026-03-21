use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

#[derive(Debug)]
pub struct BedRule {
    pub can_set_spawn: &'static str,
    pub can_sleep: &'static str,
    pub explodes: bool,
    pub error_message_key: Option<&'static str>,
}

#[derive(Debug)]
pub struct MoodSound {
    pub sound: &'static str,
    pub tick_delay: i32,
    pub block_search_extent: i32,
    pub offset: f64,
}

#[derive(Debug)]
pub struct MusicEntry {
    pub sound: &'static str,
    pub min_delay: i32,
    pub max_delay: i32,
    pub replace_current_music: bool,
}

#[derive(Debug)]
pub struct BackgroundMusic {
    pub default: MusicEntry,
    pub creative: Option<MusicEntry>,
}

/// Represents a full dimension type definition from a data pack JSON file.
#[derive(Debug)]
pub struct DimensionType {
    pub key: Identifier,
    pub fixed_time: Option<i64>,
    pub has_skylight: bool,
    pub has_ceiling: bool,
    pub coordinate_scale: f64,
    pub min_y: i32,
    pub height: i32,
    pub logical_height: i32,
    pub infiniburn: &'static str,
    pub ambient_light: f32,
    pub default_clock: Option<&'static str>,
    pub timelines: Option<&'static str>,
    pub has_ender_dragon_fight: bool,
    pub monster_spawn_light_level: MonsterSpawnLightLevel,
    pub monster_spawn_block_light_limit: i32,

    // Top-level
    pub skybox: Option<&'static str>,
    pub cardinal_light: Option<&'static str>,

    // Attributes: visual
    pub sky_color: Option<&'static str>,
    pub fog_color: Option<&'static str>,
    pub cloud_color: Option<&'static str>,
    pub cloud_height: Option<f32>,
    pub ambient_light_color: Option<&'static str>,
    pub sky_light_color: Option<&'static str>,
    pub sky_light_factor: Option<f32>,
    pub fog_start_distance: Option<f32>,
    pub fog_end_distance: Option<f32>,
    pub default_dripstone_particle: Option<&'static str>,

    // Attributes: gameplay
    pub respawn_anchor_works: bool,
    pub can_start_raid: bool,
    pub fast_lava: bool,
    pub piglins_zombify: bool,
    pub sky_light_level: Option<f32>,
    pub snow_golem_melts: bool,
    pub water_evaporates: bool,
    pub nether_portal_spawns_piglin: bool,
    pub bed_rule: BedRule,

    // Attributes: audio
    pub mood_sound: Option<MoodSound>,
    pub background_music: Option<BackgroundMusic>,
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

impl ToNbtTag for &DimensionType {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();

        // Top-level fields
        if let Some(fixed_time) = self.fixed_time {
            compound.insert("fixed_time", fixed_time);
        }
        compound.insert("has_skylight", self.has_skylight);
        compound.insert("has_ceiling", self.has_ceiling);
        compound.insert("coordinate_scale", self.coordinate_scale);
        compound.insert("min_y", self.min_y);
        compound.insert("height", self.height);
        compound.insert("logical_height", self.logical_height);
        compound.insert("infiniburn", self.infiniburn);
        compound.insert("ambient_light", self.ambient_light);
        compound.insert("has_ender_dragon_fight", self.has_ender_dragon_fight);
        if let Some(clock) = self.default_clock {
            compound.insert("default_clock", clock);
        }
        if let Some(timelines) = self.timelines {
            compound.insert("timelines", timelines);
        }
        if let Some(skybox) = self.skybox {
            compound.insert("skybox", skybox);
        }
        if let Some(cardinal_light) = self.cardinal_light {
            compound.insert("cardinal_light", cardinal_light);
        }
        compound.insert(
            "monster_spawn_light_level",
            match &self.monster_spawn_light_level {
                MonsterSpawnLightLevel::Simple(v) => NbtTag::Int(*v),
                MonsterSpawnLightLevel::Complex {
                    distribution_type,
                    min_inclusive,
                    max_inclusive,
                } => {
                    let mut inner = NbtCompound::new();
                    inner.insert("type", *distribution_type);
                    inner.insert("min_inclusive", *min_inclusive);
                    inner.insert("max_inclusive", *max_inclusive);
                    NbtTag::Compound(inner)
                }
            },
        );
        compound.insert(
            "monster_spawn_block_light_limit",
            self.monster_spawn_block_light_limit,
        );

        // Attributes compound
        let mut attributes = NbtCompound::new();

        // Visual attributes
        if let Some(sky_color) = self.sky_color {
            attributes.insert("minecraft:visual/sky_color", sky_color);
        }
        if let Some(fog_color) = self.fog_color {
            attributes.insert("minecraft:visual/fog_color", fog_color);
        }
        if let Some(cloud_color) = self.cloud_color {
            attributes.insert("minecraft:visual/cloud_color", cloud_color);
        }
        if let Some(cloud_height) = self.cloud_height {
            attributes.insert("minecraft:visual/cloud_height", cloud_height);
        }
        if let Some(ambient_light_color) = self.ambient_light_color {
            attributes.insert("minecraft:visual/ambient_light_color", ambient_light_color);
        }
        if let Some(sky_light_color) = self.sky_light_color {
            attributes.insert("minecraft:visual/sky_light_color", sky_light_color);
        }
        if let Some(sky_light_factor) = self.sky_light_factor {
            attributes.insert("minecraft:visual/sky_light_factor", sky_light_factor);
        }
        if let Some(fog_start_distance) = self.fog_start_distance {
            attributes.insert("minecraft:visual/fog_start_distance", fog_start_distance);
        }
        if let Some(fog_end_distance) = self.fog_end_distance {
            attributes.insert("minecraft:visual/fog_end_distance", fog_end_distance);
        }
        if let Some(particle_type) = self.default_dripstone_particle {
            let mut particle = NbtCompound::new();
            particle.insert("type", particle_type);
            attributes.insert(
                "minecraft:visual/default_dripstone_particle",
                NbtTag::Compound(particle),
            );
        }

        // Gameplay attributes
        attributes.insert(
            "minecraft:gameplay/respawn_anchor_works",
            self.respawn_anchor_works,
        );
        attributes.insert("minecraft:gameplay/can_start_raid", self.can_start_raid);
        if self.fast_lava {
            attributes.insert("minecraft:gameplay/fast_lava", self.fast_lava);
        }
        if !self.piglins_zombify {
            attributes.insert("minecraft:gameplay/piglins_zombify", self.piglins_zombify);
        }
        if let Some(sky_light_level) = self.sky_light_level {
            attributes.insert("minecraft:gameplay/sky_light_level", sky_light_level);
        }
        if self.snow_golem_melts {
            attributes.insert("minecraft:gameplay/snow_golem_melts", self.snow_golem_melts);
        }
        if self.water_evaporates {
            attributes.insert("minecraft:gameplay/water_evaporates", self.water_evaporates);
        }
        if self.nether_portal_spawns_piglin {
            attributes.insert(
                "minecraft:gameplay/nether_portal_spawns_piglin",
                self.nether_portal_spawns_piglin,
            );
        }

        // Bed rule
        {
            let mut bed_rule = NbtCompound::new();
            bed_rule.insert("can_set_spawn", self.bed_rule.can_set_spawn);
            bed_rule.insert("can_sleep", self.bed_rule.can_sleep);
            if self.bed_rule.explodes {
                bed_rule.insert("explodes", self.bed_rule.explodes);
            }
            if let Some(key) = self.bed_rule.error_message_key {
                let mut msg = NbtCompound::new();
                msg.insert("translate", key);
                bed_rule.insert("error_message", NbtTag::Compound(msg));
            }
            attributes.insert("minecraft:gameplay/bed_rule", NbtTag::Compound(bed_rule));
        }

        // Audio attributes
        if let Some(mood) = &self.mood_sound {
            let mut mood_compound = NbtCompound::new();
            mood_compound.insert("sound", mood.sound);
            mood_compound.insert("tick_delay", mood.tick_delay);
            mood_compound.insert("block_search_extent", mood.block_search_extent);
            mood_compound.insert("offset", mood.offset);
            let mut ambient_sounds = NbtCompound::new();
            ambient_sounds.insert("mood", NbtTag::Compound(mood_compound));
            attributes.insert(
                "minecraft:audio/ambient_sounds",
                NbtTag::Compound(ambient_sounds),
            );
        }
        if let Some(bg_music) = &self.background_music {
            let mut music_compound = NbtCompound::new();
            let mut default_entry = NbtCompound::new();
            default_entry.insert("sound", bg_music.default.sound);
            default_entry.insert("min_delay", bg_music.default.min_delay);
            default_entry.insert("max_delay", bg_music.default.max_delay);
            if bg_music.default.replace_current_music {
                default_entry.insert(
                    "replace_current_music",
                    bg_music.default.replace_current_music,
                );
            }
            music_compound.insert("default", NbtTag::Compound(default_entry));
            if let Some(creative) = &bg_music.creative {
                let mut creative_entry = NbtCompound::new();
                creative_entry.insert("sound", creative.sound);
                creative_entry.insert("min_delay", creative.min_delay);
                creative_entry.insert("max_delay", creative.max_delay);
                if creative.replace_current_music {
                    creative_entry.insert("replace_current_music", creative.replace_current_music);
                }
                music_compound.insert("creative", NbtTag::Compound(creative_entry));
            }
            attributes.insert(
                "minecraft:audio/background_music",
                NbtTag::Compound(music_compound),
            );
        }

        if !attributes.is_empty() {
            compound.insert("attributes", NbtTag::Compound(attributes));
        }

        NbtTag::Compound(compound)
    }
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
