//! entity specific state chosen by world gen before a mob is constructed

use steel_registry::DyeColor;
use steel_registry::chicken_sound_variant::ChickenSoundVariantRef;
use steel_registry::chicken_variant::ChickenVariantRef;
use steel_registry::cow_sound_variant::CowSoundVariantRef;
use steel_registry::cow_variant::CowVariantRef;
use steel_registry::entity_variant::{
    FoxVariant, HorseVariant, LlamaVariant, ParrotVariant, RabbitVariant,
};
use steel_registry::frog_variant::FrogVariantRef;
use steel_registry::pig_sound_variant::PigSoundVariantRef;
use steel_registry::pig_variant::PigVariantRef;
use steel_registry::wolf_sound_variant::WolfSoundVariantRef;
use steel_registry::wolf_variant::WolfVariantRef;
use steel_utils::BlockPos;

pub use steel_registry::entity_variant::{HorseMarkings, PandaGene};

/// state specific to an entity impl like common aging attributes and equipment are applied by the worldgen spawner
#[derive(Debug, Default, Clone, Copy)]
#[expect(
    missing_docs,
    reason = "fields correspond directly to named vanilla entity state"
)]
pub enum WorldgenSpawnProperties {
    #[default]
    None,
    Camel {
        last_pose_change_tick: i64,
    },
    Chicken {
        variant: Option<ChickenVariantRef>,
        sound_variant: ChickenSoundVariantRef,
    },
    Cow {
        variant: Option<CowVariantRef>,
        sound_variant: CowSoundVariantRef,
    },
    Fox(FoxVariant),
    Frog {
        variant: Option<FrogVariantRef>,
        long_jump_cooldown_ticks: i32,
    },
    Goat {
        long_jump_cooldown_ticks: i32,
        ram_cooldown_ticks: i32,
        screaming: bool,
        missing_left_horn: Option<bool>,
    },
    Horse {
        variant: HorseVariant,
        markings: HorseMarkings,
    },
    Llama {
        variant: LlamaVariant,
        strength: i32,
    },
    Panda {
        main_gene: PandaGene,
        hidden_gene: PandaGene,
    },
    Parrot(ParrotVariant),
    Pig {
        variant: Option<PigVariantRef>,
        sound_variant: PigSoundVariantRef,
    },
    Rabbit(RabbitVariant),
    Sheep(DyeColor),
    Turtle {
        home: BlockPos,
    },
    Wolf {
        variant: Option<WolfVariantRef>,
        sound_variant: WolfSoundVariantRef,
    },
    ZombifiedPiglin {
        can_break_doors: bool,
        difficulty_multiplier: f32,
    },
}
