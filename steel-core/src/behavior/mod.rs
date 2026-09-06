//! Block and item behavior system.
//!
//! This module contains the behavior traits and registries that define how
//! blocks and items behave dynamically. This is separate from the static data
//! in steel-registry to maintain a clean separation between constant data and
//! functional/dynamic behavior.
//!
//! # Architecture
//!
//! After the main registry (`steel-registry`) is frozen, behavior registries
//! are created:
//! - `BlockBehaviorRegistry` - assigns default or custom behaviors to each block
//! - `ItemBehaviorRegistry` - assigns default or custom behaviors to each item
//!
//! # Usage
//!
//! ```ignore
//! use steel_core::behavior::{init_behaviors, BLOCK_BEHAVIORS, ITEM_BEHAVIORS};
//!
//! // After registry is frozen, call once at startup:
//! init_behaviors();
//!
//! // Then access behaviors via the global registries:
//! let behavior = BLOCK_BEHAVIORS.get_behavior(block);
//! ```

mod block;
pub mod blocks;
mod consume_effect;
mod context;
pub mod fluid;
mod item;
pub(crate) mod item_utils;
pub mod items;
mod mob_effect;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/blocks.rs"]
pub mod block_behaviors;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/candle_cakes.rs"]
pub mod candle_cakes;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/items.rs"]
pub mod item_behaviors;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/strippables.rs"]
pub mod strippables;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/waxables.rs"]
pub mod waxables;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/weathering.rs"]
pub mod weathering;

pub use block::{
    BlockBehavior, BlockBehaviorRegistry, BlockCollisionBoxes, BlockCollisionContext,
    BlockEntityCreation, BlockLootContext, BrushableData, DefaultBlockBehavior, EntityFallDamage,
    EntityFallOnContext, EntityFallOnFacts, EntityLandingContext, Fallable, RailBehavior,
};
pub(crate) use block::{pickup_waterlogged_block, try_drop_experience};
use block_behaviors::register_block_behaviors;
pub use consume_effect::{CONSUME_EFFECT_BEHAVIORS, ConsumeEffectBehaviorRegistry};
pub use context::{
    BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess, PlacementOrientation,
    PlacementSource, UseItemContext, UseOnContext,
};
pub use fluid::{FLUID_BEHAVIORS, FluidBehaviorRegistry};
pub use item::{ItemBehavior, ItemBehaviorRegistry, ItemUseAnimation};
use item_behaviors::register_item_behaviors;
pub use items::{
    BedItem, BlockItem, BucketItem, DefaultItemBehavior, DoubleHighBlockItem, EnderEyeItem,
    HangingSignItem, ScaffoldingBlockItem, ShovelItem, SignItem, SolidBucketItem,
    StandingAndWallBlockItem,
};
pub use mob_effect::{MOB_EFFECT_BEHAVIORS, MobEffectBehaviorRegistry};
use std::ops::Deref;
use std::sync::OnceLock;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::consume_effect::vanilla_consume_effect_types;
use steel_registry::vanilla_fluids;
use steel_registry::vanilla_mob_effects;
use steel_utils::BlockStateId;

use crate::entity::ai::path::PathComputationType;
use crate::entity::consume_effect::{
    ApplyEffectsBehavior, ClearAllEffectsBehavior, ConsumeEffectBehavior, PlaySoundBehavior,
    RemoveEffectsBehavior, TeleportRandomlyBehavior,
};
use crate::entity::mob_effect::{
    AbsorptionBehavior, BadOmenBehavior, HealOrHarmBehavior, HungerBehavior, InfestedBehavior,
    MobEffectBehavior, OozingBehavior, PoisonBehavior, RaidOmenBehavior, RegenerationBehavior,
    SaturationBehavior, WeavingBehavior, WindChargedBehavior, WitherBehavior,
};
use crate::fluid::{FluidBehavior, LavaFluid, WaterFluid};

/// Wrapper for the global block behavior registry that implements `Deref`.
pub struct BlockBehaviorLock(OnceLock<BlockBehaviorRegistry>);

impl Deref for BlockBehaviorLock {
    type Target = BlockBehaviorRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Block behaviors not initialized")
    }
}

/// Wrapper for the global item behavior registry that implements `Deref`.
pub struct ItemBehaviorLock(OnceLock<ItemBehaviorRegistry>);

impl Deref for ItemBehaviorLock {
    type Target = ItemBehaviorRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Item behaviors not initialized")
    }
}

/// Extension trait for `BlockStateId` that provides access to behavior-dependent methods.
///
/// This is separate from `BlockStateExt` (in steel-registry) because these methods
/// require access to the behavior registry which lives in steel-core.
pub trait BlockStateBehaviorExt {
    /// Returns whether this block state belongs to a vanilla `LiquidBlockContainer`.
    fn is_liquid_container(&self) -> bool;

    /// Returns whether this block state can be replaced by the given fluid block.
    fn can_be_replaced_by_fluid(&self, fluid_block: BlockRef) -> bool;

    /// Returns whether this block state can be replaced in this placement context.
    fn can_be_replaced(&self, context: &BlockPlaceContext<'_>) -> bool;

    /// Returns whether this block state is pathfindable for the supplied vanilla computation type.
    fn is_pathfindable(&self, computation_type: PathComputationType) -> bool;

    /// Returns whether this block state extends `BedBlock`
    fn is_bed(&self) -> bool;

    /// Returns whether this block state can be occupied by a forced respawn position
    fn is_possible_to_respawn_in_this(&self) -> bool;
}

impl BlockStateBehaviorExt for BlockStateId {
    fn is_liquid_container(&self) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.is_liquid_container(*self)
    }

    fn can_be_replaced_by_fluid(&self, fluid_block: BlockRef) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.can_be_replaced_by_fluid(*self, fluid_block)
    }

    fn can_be_replaced(&self, context: &BlockPlaceContext<'_>) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.can_be_replaced(*self, context)
    }

    fn is_pathfindable(&self, computation_type: PathComputationType) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.is_pathfindable(*self, computation_type)
    }

    fn is_bed(&self) -> bool {
        let block = self.get_block();
        BLOCK_BEHAVIORS.get_behavior(block).is_bed()
    }

    fn is_possible_to_respawn_in_this(&self) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.is_possible_to_respawn_in_this(*self)
    }
}

/// Global block behavior registry.
///
/// Access behaviors directly via deref: `BLOCK_BEHAVIORS.get_behavior(block)`
pub static BLOCK_BEHAVIORS: BlockBehaviorLock = BlockBehaviorLock(OnceLock::new());

/// Global item behavior registry.
///
/// Access behaviors directly via deref: `ITEM_BEHAVIORS.get_behavior(item)`
pub static ITEM_BEHAVIORS: ItemBehaviorLock = ItemBehaviorLock(OnceLock::new());

/// Initializes the global behavior registries.
///
/// This should be called after the main registry is frozen. Repeated calls are a no-op.
pub fn init_behaviors() {
    BLOCK_BEHAVIORS.0.get_or_init(|| {
        let mut block_behaviors = BlockBehaviorRegistry::new();
        register_block_behaviors(&mut block_behaviors);
        block_behaviors
    });

    FLUID_BEHAVIORS.0.get_or_init(|| {
        let mut fluid_behaviors = FluidBehaviorRegistry::new();

        // Water: WaterFluid implements FluidBehavior directly
        let water_behavior: Box<dyn FluidBehavior> = Box::new(WaterFluid);
        // Both WATER and FLOWING_WATER share the same behavior
        fluid_behaviors.set_behavior(&vanilla_fluids::WATER, water_behavior);
        fluid_behaviors.set_behavior(&vanilla_fluids::FLOWING_WATER, Box::new(WaterFluid));

        // Lava: LavaFluid implements FluidBehavior directly
        let lava_behavior: Box<dyn FluidBehavior> = Box::new(LavaFluid);
        fluid_behaviors.set_behavior(&vanilla_fluids::LAVA, lava_behavior);
        fluid_behaviors.set_behavior(&vanilla_fluids::FLOWING_LAVA, Box::new(LavaFluid));

        fluid_behaviors
    });

    ITEM_BEHAVIORS.0.get_or_init(|| {
        let mut item_behaviors = ItemBehaviorRegistry::new();
        register_item_behaviors(&mut item_behaviors);
        item_behaviors
    });

    CONSUME_EFFECT_BEHAVIORS.0.get_or_init(|| {
        let mut consume_effect_behaviors = ConsumeEffectBehaviorRegistry::new();

        let apply_effects: Box<dyn ConsumeEffectBehavior> = Box::new(ApplyEffectsBehavior);
        consume_effect_behaviors
            .set_behavior(&vanilla_consume_effect_types::APPLY_EFFECTS, apply_effects);

        let remove_effects: Box<dyn ConsumeEffectBehavior> = Box::new(RemoveEffectsBehavior);
        consume_effect_behaviors.set_behavior(
            &vanilla_consume_effect_types::REMOVE_EFFECTS,
            remove_effects,
        );

        let clear_all_effects: Box<dyn ConsumeEffectBehavior> = Box::new(ClearAllEffectsBehavior);
        consume_effect_behaviors.set_behavior(
            &vanilla_consume_effect_types::CLEAR_ALL_EFFECTS,
            clear_all_effects,
        );

        let play_sound: Box<dyn ConsumeEffectBehavior> = Box::new(PlaySoundBehavior);
        consume_effect_behaviors
            .set_behavior(&vanilla_consume_effect_types::PLAY_SOUND, play_sound);

        let teleport_randomly: Box<dyn ConsumeEffectBehavior> = Box::new(TeleportRandomlyBehavior);
        consume_effect_behaviors.set_behavior(
            &vanilla_consume_effect_types::TELEPORT_RANDOMLY,
            teleport_randomly,
        );

        consume_effect_behaviors
    });

    MOB_EFFECT_BEHAVIORS.0.get_or_init(|| {
        let mut mob_effect_behaviors = MobEffectBehaviorRegistry::new();

        // HealOrHarmMobEffect backs both Instant Health and Instant Damage.
        let instant_health: Box<dyn MobEffectBehavior> =
            Box::new(HealOrHarmBehavior { is_harm: false });
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::INSTANT_HEALTH, instant_health);

        let instant_damage: Box<dyn MobEffectBehavior> =
            Box::new(HealOrHarmBehavior { is_harm: true });
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::INSTANT_DAMAGE, instant_damage);

        let saturation: Box<dyn MobEffectBehavior> = Box::new(SaturationBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::SATURATION, saturation);

        let wither: Box<dyn MobEffectBehavior> = Box::new(WitherBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::WITHER, wither);

        let poison: Box<dyn MobEffectBehavior> = Box::new(PoisonBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::POISON, poison);

        let regeneration: Box<dyn MobEffectBehavior> = Box::new(RegenerationBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::REGENERATION, regeneration);

        let hunger: Box<dyn MobEffectBehavior> = Box::new(HungerBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::HUNGER, hunger);

        let absorption: Box<dyn MobEffectBehavior> = Box::new(AbsorptionBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::ABSORPTION, absorption);

        let bad_omen: Box<dyn MobEffectBehavior> = Box::new(BadOmenBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::BAD_OMEN, bad_omen);

        let raid_omen: Box<dyn MobEffectBehavior> = Box::new(RaidOmenBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::RAID_OMEN, raid_omen);

        let oozing: Box<dyn MobEffectBehavior> = Box::new(OozingBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::OOZING, oozing);

        let weaving: Box<dyn MobEffectBehavior> = Box::new(WeavingBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::WEAVING, weaving);

        let wind_charged: Box<dyn MobEffectBehavior> = Box::new(WindChargedBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::WIND_CHARGED, wind_charged);

        let infested: Box<dyn MobEffectBehavior> = Box::new(InfestedBehavior);
        mob_effect_behaviors.set_behavior(vanilla_mob_effects::INFESTED, infested);

        mob_effect_behaviors
    });
}
