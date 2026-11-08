use glam::DVec3;
use steel_registry::biome::BiomeRef;
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::entity_type::MobCategory;
use steel_registry::environment_attribute::TriState;
use steel_registry::init_vanilla_registry;
use steel_registry::vanilla_dimension_types::{OVERWORLD, THE_NETHER};
use steel_registry::vanilla_entities;
use steel_registry::vanilla_environment_attributes::{
    EYEBLOSSOM_OPEN, FAST_LAVA, INCREASED_FIRE_BURNOUT, MONSTERS_BURN, NATURAL_MOB_SPAWNS,
    SKY_LIGHT_LEVEL, SUN_ANGLE,
};
use steel_registry::{vanilla_biomes, vanilla_world_clocks};
use steel_utils::BlockPos;
use steel_utils::value_providers::IntProvider;

use super::{BiomeSource, EnvironmentAttributeSystem, EnvironmentSampleContext};
use crate::world::clock::WorldClockManager;

const F32_CLOSE_EPSILON: f32 = 0.000_001;
const OVERWORLD_WAKE_UP_TICKS: i64 = 0;
const OVERWORLD_DAY_TICKS: i64 = 1_000;
const OVERWORLD_NOON_TICKS: i64 = 6_000;
const OVERWORLD_SUNSET_TICKS: i64 = 12_000;
const OVERWORLD_SUNSET_INTERPOLATION_TICKS: i64 = 12_768;
const OVERWORLD_MIDNIGHT_TICKS: i64 = 18_000;
const ORIGIN: DVec3 = DVec3::new(0.5, 64.0, 0.5);

/// Resolves every position to one biome, or to none.
struct FixedBiome(Option<BiomeRef>);

impl BiomeSource for FixedBiome {
    fn noise_biome(&self, _quart_x: i32, _quart_y: i32, _quart_z: i32) -> Option<BiomeRef> {
        self.0
    }

    fn biome(&self, _pos: BlockPos) -> Option<BiomeRef> {
        self.0
    }
}

const NO_BIOME: FixedBiome = FixedBiome(None);

struct Fixture {
    system: EnvironmentAttributeSystem,
    clocks: WorldClockManager,
}

impl Fixture {
    fn new(dimension_type: DimensionTypeRef, can_have_weather: bool, ticks: i64) -> Self {
        init_vanilla_registry();
        let mut clocks = WorldClockManager::new();
        assert_eq!(
            clocks.set_total_ticks(&vanilla_world_clocks::OVERWORLD, ticks),
            Some(())
        );
        Self {
            system: EnvironmentAttributeSystem::builder()
                .add_default_layers(dimension_type, can_have_weather)
                .build(),
            clocks,
        }
    }

    fn overworld(ticks: i64) -> Self {
        Self::new(&OVERWORLD, true, ticks)
    }

    fn context<'a>(
        &'a self,
        rain_level: f32,
        thunder_level: f32,
        biomes: &'a dyn BiomeSource,
    ) -> EnvironmentSampleContext<'a> {
        EnvironmentSampleContext {
            clocks: &self.clocks,
            rain_level,
            thunder_level,
            biomes,
        }
    }

    fn sky_light_level(&self, rain_level: f32, thunder_level: f32) -> f32 {
        self.system.get_dimension_value(
            &SKY_LIGHT_LEVEL,
            &self.context(rain_level, thunder_level, &NO_BIOME),
        )
    }

    fn monsters_burn(&self) -> bool {
        self.system
            .get_value(&MONSTERS_BURN, ORIGIN, &self.context(0.0, 0.0, &NO_BIOME))
    }
}

fn assert_f32_close(left: f32, right: f32) {
    assert!(
        (left - right).abs() < F32_CLOSE_EPSILON,
        "left={left}, right={right}"
    );
}

#[test]
fn overworld_sky_light_uses_generated_day_timeline() {
    assert_f32_close(
        Fixture::overworld(OVERWORLD_DAY_TICKS).sky_light_level(0.0, 0.0),
        15.0,
    );
    assert_f32_close(
        Fixture::overworld(OVERWORLD_NOON_TICKS).sky_light_level(0.0, 0.0),
        15.0,
    );
    assert_f32_close(
        Fixture::overworld(OVERWORLD_MIDNIGHT_TICKS).sky_light_level(0.0, 0.0),
        4.0,
    );
}

#[test]
fn overworld_sky_light_interpolates_sunset_from_generated_keyframes() {
    assert_f32_close(
        Fixture::overworld(OVERWORLD_SUNSET_INTERPOLATION_TICKS).sky_light_level(0.0, 0.0),
        9.503_051,
    );
}

#[test]
fn overworld_sun_angle_uses_vanilla_cubic_bezier_easing() {
    let sun_angle = |ticks| {
        let fixture = Fixture::overworld(ticks);
        fixture
            .system
            .get_value(&SUN_ANGLE, ORIGIN, &fixture.context(0.0, 0.0, &NO_BIOME))
    };
    assert_f32_close(sun_angle(OVERWORLD_WAKE_UP_TICKS), 282.374_33);
    assert_f32_close(sun_angle(OVERWORLD_SUNSET_TICKS), 77.625_66);
    assert_f32_close(sun_angle(OVERWORLD_MIDNIGHT_TICKS), 180.0);
}

#[test]
fn sky_light_level_applies_vanilla_weather_alpha_layers() {
    let noon = Fixture::overworld(OVERWORLD_NOON_TICKS);
    assert_f32_close(noon.sky_light_level(1.0, 0.0), 11.5625);
    assert_f32_close(noon.sky_light_level(1.0, 1.0), 9.199_219);
}

#[test]
fn dimensions_without_weather_ignore_weather_levels() {
    let nether = Fixture::new(&THE_NETHER, false, OVERWORLD_NOON_TICKS);
    assert_f32_close(nether.sky_light_level(1.0, 1.0), 4.0);
}

#[test]
fn monsters_burn_follows_the_generated_daylight_window() {
    // Daylight spans the period wrap: sunrise at 23460, sunset at 12542.
    for ticks in [0, 6_000, 12_000, 12_541, 23_460, 23_999] {
        assert!(
            Fixture::overworld(ticks).monsters_burn(),
            "monsters_burn should hold at tick {ticks}"
        );
    }
    for ticks in [12_542, 18_000, 23_000, 23_459] {
        assert!(
            !Fixture::overworld(ticks).monsters_burn(),
            "monsters_burn should be off at tick {ticks}"
        );
    }
}

#[test]
fn monsters_burn_is_off_in_dimensions_without_the_day_timeline() {
    assert!(!Fixture::new(&THE_NETHER, false, OVERWORLD_NOON_TICKS).monsters_burn());
}

#[test]
fn dimension_constant_layer_sets_fast_lava() {
    let fast_lava = |dimension_type, can_have_weather| {
        let fixture = Fixture::new(dimension_type, can_have_weather, OVERWORLD_NOON_TICKS);
        fixture
            .system
            .get_dimension_value(&FAST_LAVA, &fixture.context(0.0, 0.0, &NO_BIOME))
    };
    assert!(fast_lava(&THE_NETHER, false));
    assert!(!fast_lava(&OVERWORLD, true));
}

#[test]
fn biome_layer_reads_the_biome_at_the_sampled_position() {
    let fixture = Fixture::overworld(OVERWORLD_NOON_TICKS);
    let burnout = |biome: Option<BiomeRef>| {
        fixture.system.get_value(
            &INCREASED_FIRE_BURNOUT,
            ORIGIN,
            &fixture.context(0.0, 0.0, &FixedBiome(biome)),
        )
    };
    assert!(burnout(Some(&vanilla_biomes::JUNGLE)));
    assert!(!burnout(Some(&vanilla_biomes::PLAINS)));
    assert!(!burnout(None));
}

#[test]
fn eyeblossom_open_holds_tri_state_keyframes_of_the_day_timeline() {
    let eyeblossom_open = |ticks| {
        let fixture = Fixture::overworld(ticks);
        fixture.system.get_value(
            &EYEBLOSSOM_OPEN,
            ORIGIN,
            &fixture.context(0.0, 0.0, &NO_BIOME),
        )
    };
    // Open from tick 12600, closed from tick 23401, wrapping over the day.
    assert_eq!(eyeblossom_open(OVERWORLD_NOON_TICKS), TriState::False);
    assert_eq!(eyeblossom_open(OVERWORLD_MIDNIGHT_TICKS), TriState::True);
    assert_eq!(eyeblossom_open(23_401), TriState::False);
}

#[test]
fn biome_natural_mob_spawns_overlay_the_empty_default() {
    let fixture = Fixture::overworld(OVERWORLD_NOON_TICKS);
    let spawns = fixture.system.get_value(
        &NATURAL_MOB_SPAWNS,
        ORIGIN,
        &fixture.context(0.0, 0.0, &FixedBiome(Some(&vanilla_biomes::PLAINS))),
    );

    let creatures = spawns.get_mobs_to_spawn(MobCategory::Creature);
    assert_eq!(
        creatures[0].value.entity_type().key,
        vanilla_entities::SHEEP.key
    );
    assert_eq!(creatures[0].weight, 12);
    assert!(matches!(
        creatures[0].value.count(),
        IntProvider::Constant(4)
    ));

    let glow_squid = &spawns.get_mobs_to_spawn(MobCategory::UndergroundWaterCreature)[0];
    assert!(matches!(
        glow_squid.value.count(),
        IntProvider::Uniform {
            min_inclusive: 4,
            max_inclusive: 6
        }
    ));
    assert!(spawns.get_mobs_in_category(MobCategory::Axolotls).is_none());
}
