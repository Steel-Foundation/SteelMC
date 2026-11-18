
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::level_events;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_items;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, TaggedRegistryExt};
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use crate::entity::entities::{
    LingeringPotionEntity, SnowballEntity, SplashPotionEntity, ThrownEggEntity,
};
use crate::entity::projectile::ThrowableItemProjectile;
use crate::world::World;

const DEFAULT_PROJECTILE_POWER: f32 = 1.1;
const DEFAULT_PROJECTILE_SPREAD: f32 = 6.0;
const POTION_PROJECTILE_POWER: f32 = 1.375;
const POTION_PROJECTILE_SPREAD: f32 = 3.0;

pub mod armor;
pub mod bucket;
pub mod consumables;
pub mod container;
pub mod default;
pub mod potion;
pub mod projectile;
pub mod tnt;
pub mod tools;

};
pub(crate) use default::eject_item;
pub use potion::{GlassBottleDispenseBehavior, PotionDispenseBehavior};
pub use projectile::ProjectileDispenseBehavior;
pub use tnt::TntDispenseBehavior;
pub use tools::FlintAndSteelDispenseBehavior;

pub(crate) enum DispenseResult {
    Success,
    Failure,
    Projectile,
    Byproduct(ItemStack),
}

impl DispenseResult {
    pub(crate) const fn sound(&self) -> i32 {
        match self {
            Self::Success | Self::Byproduct(_) => level_events::SOUND_DISPENSER_DISPENSE,
            Self::Failure => level_events::SOUND_DISPENSER_FAIL,
            Self::Projectile => level_events::SOUND_DISPENSER_PROJECTILE_LAUNCH,
        }
    }
}

pub(crate) fn play_feedback(world: &World, pos: BlockPos, state: BlockStateId, sound: i32) {
    let facing = state.get_value(super::dispenser_block::FACING);
    world.level_event(sound, pos, 0, None);
    world.level_event(
        level_events::PARTICLES_SHOOT_SMOKE,
        pos,
        facing.get_3d_data_value(),
        None,
    );
}

fn nested_default_dispense(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    item: &mut ItemStack,
) -> DispenseResult {
    let outcome = DefaultDispenseBehavior.dispense(world, pos, state, item);
    play_feedback(world, pos, state, outcome.sound());

    outcome
}

pub(crate) trait DispenseItemBehavior: Send + Sync {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult;
}

pub(crate) struct DispenseBehaviorRegistry {
    behaviors: Vec<Option<Box<dyn DispenseItemBehavior>>>,
}

impl DispenseBehaviorRegistry {
    pub fn new() -> Self {
        let item_count = REGISTRY.items.len();
        let behaviors = (0..item_count).map(|_| None).collect();

        Self { behaviors }
    }

    self.behaviors[id] = Some(behavior);
    }

    pub fn get_behavior(&self, stack: &ItemStack) -> &dyn DispenseItemBehavior {
        if let Some(behavior) = self.behaviors[stack.item().id()].as_deref() {
            behavior
        } else if stack.get_equippable().is_some() {
            &ArmorDispenseBehavior
        } else {
            &DefaultDispenseBehavior
        }
    }
}

pub(crate) static DISPENSE_BEHAVIORS: LazyLock<DispenseBehaviorRegistry> = LazyLock::new(|| {
    let mut registry = DispenseBehaviorRegistry::new();

    // TODO: register arrow dispense behaviors once the arrow entities are available

    // boat
    // TODO: boats and rafts - need the boat entities.

    // bucket
    registry.set_behavior(
        &vanilla_items::WATER_BUCKET,
        Box::new(FilledBucketDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::LAVA_BUCKET,
        Box::new(FilledBucketDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::POWDER_SNOW_BUCKET,
        Box::new(FilledBucketDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::BUCKET,
        Box::new(EmptyBucketDispenseBehavior),
    );
    // TODO: mob buckets (salmon, cod, pufferfish, tropical fish, axolotl,
    // sulfur cube, tadpole) — vanilla empties them into a mob
    // entity at the target block, but none of those entities/spawn paths
    // are ported yet. Currently fall through to DefaultDispenseBehavior.

    // consumables
    registry.set_behavior(
        &vanilla_items::BONE_MEAL,
        Box::new(BoneMealDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::HONEYCOMB,
        Box::new(HoneycombDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::GLOWSTONE,
        Box::new(GlowstoneDispenseBehavior),
    );
    // TODO: carved pumpkin - see consumables.rs
    // TODO: wither skeleton skull - needs the Wither entity for the summon pattern.

    // potion
    registry.set_behavior(
        &vanilla_items::GLASS_BOTTLE,
        Box::new(GlassBottleDispenseBehavior),
    );
    registry.set_behavior(&vanilla_items::POTION, Box::new(PotionDispenseBehavior));

    // projectile
    for egg in [
        &vanilla_items::EGG,
        &vanilla_items::BLUE_EGG,
        &vanilla_items::BROWN_EGG,
    ] {
        registry.set_behavior(
            ProjectileDispenseBehavior::new(
                    &vanilla_entities::EGG,
                    DEFAULT_PROJECTILE_POWER,
                    DEFAULT_PROJECTILE_SPREAD,
                )
                .with_item_setter(|entity, item| {
                    if let Some(egg) = entity.downcast_ref::<ThrownEggEntity>() {
                        egg.set_item_clamped(item.clone());
                    }
                }),
            ),
        );
    }
    registry.set_behavior(
        &vanilla_items::SNOWBALL,
        Box::new(
            ProjectileDispenseBehavior::new(
                &vanilla_entities::SNOWBALL,
                DEFAULT_PROJECTILE_POWER,
                DEFAULT_PROJECTILE_SPREAD,
            )
            .with_item_setter(|entity, item| {
                if let Some(snowball) = entity.downcast_ref::<SnowballEntity>() {
                    snowball.set_item_clamped(item.clone());
                }
            }),
        ),
    );

    registry.set_behavior(
        &vanilla_items::SPLASH_POTION,
        Box::new(
            ProjectileDispenseBehavior::new(
                &vanilla_entities::SPLASH_POTION,
                POTION_PROJECTILE_POWER,
                POTION_PROJECTILE_SPREAD,
            )
            .with_item_setter(|entity, item| {
                if let Some(potion) = entity.downcast_ref::<SplashPotionEntity>() {
                    potion.set_item_clamped(item.clone());
                }
            }),
        ),
    );

    registry.set_behavior(
        ProjectileDispenseBehavior::new(
                &vanilla_entities::LINGERING_POTION,
                POTION_PROJECTILE_POWER,
                POTION_PROJECTILE_SPREAD,
            )
            .with_item_setter(|entity, item| {
                if let Some(potion) = entity.downcast_ref::<LingeringPotionEntity>() {
                    potion.set_item_clamped(item.clone());
                }
            }),
        ),
    );

    // TODO: Experience bottle and fire charge need their projectile entities.
    // wind charge and firework still need their dispenser launch config

    // explosives
    registry.set_behavior(&vanilla_items::TNT, Box::new(TntDispenseBehavior));

    // tools
    registry.set_behavior(
        &vanilla_items::FLINT_AND_STEEL,
        Box::new(FlintAndSteelDispenseBehavior),
    );
    // TODO: shears, brush - see tools.rs

    // container
    for item in REGISTRY.items.iter_tag(&ItemTag::SHULKER_BOXES) {
        let Some(block) = REGISTRY.blocks.by_key(&item.key) else {
            continue;
        };
        registry.set_behavior(item, Box::new(ShulkerBoxDispenseBehavior::new(block)));
    }
    // TODO: chest - equips tamed donkeys/mules/llamas; needs those entities.

    // minecart
    // TODO: minecart and chest minecart - entities exist; port
    // MinecartDispenseItemBehavior. Furnace/hopper/tnt/command-block
    // minecarts also need their entities.

    // spawn egg
    // TODO: Dispense spawn eggs with ENTITY_DATA using their configured entity

    registry
});
