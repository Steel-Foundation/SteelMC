//! Vanilla `Minecart`: the rideable minecart.

use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::MinecartEntityData;
use steel_registry::vanilla_items;
use steel_utils::BlockPos;
use steel_utils::locks::CheckedMutex;

use super::abstract_minecart::{self, AbstractMinecartBase, AbstractMinecartProps};
use super::vehicle_entity::{self, HURT_DURATION, Parts as _};
use crate::behavior::InteractionResult;
use crate::entity::component::definition::{Definition, InteractCtx};
use crate::entity::component::pipeline::{Flow, Pipeline, Stage, StageId};
use crate::entity::prelude::*;
use crate::entity::{EntityBase, EntityProps};
use crate::player::Player;
use crate::world::World;

/// The damage an activator rail deals, enough to rock the cart without
/// breaking it.
const ACTIVATOR_RAIL_DAMAGE: f32 = 50.0;

/// A rideable minecart.
// TODO: implement getPickResult.
#[entity_behavior(class = "Minecart")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, key = "steel:entity/minecart", constructors, definition = Self::define)]
pub struct MinecartEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<MinecartEntityData>,
    #[entity(component)]
    minecart: AbstractMinecartBase,
}

impl vehicle_entity::Parts for MinecartEntity {
    fn drop_item(&self) -> ItemRef {
        &vanilla_items::MINECART
    }
}

impl MinecartEntity {
    /// The `Minecart.interact` stage, which replaces `Entity`'s.
    pub const INTERACT: StageId = StageId("minecart.interact");

    const ENTITY: EntityProps<Self> = abstract_minecart::entity_props();

    const MINECART: AbstractMinecartProps<Self> = AbstractMinecartProps {
        is_rideable: |_| true,
        activate_minecart: Self::activate_minecart,
        ..AbstractMinecartProps::new()
    };

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(MinecartEntityData::new()),
            minecart: AbstractMinecartBase::new(&Self::MINECART),
        }
    }

    fn define() -> Definition<Self> {
        fn interact(minecart: &MinecartEntity, ctx: &mut InteractCtx<'_>) -> Flow {
            ctx.result = minecart.interact_minecart(ctx.player);
            Flow::Continue
        }
        Definition {
            interact: Pipeline::new([Stage::new(MinecartEntity::INTERACT, interact)]),
            ..abstract_minecart::definition()
        }
    }

    fn interact_minecart(&self, player: &Player) -> InteractionResult {
        if player.is_secondary_use_active() || self.is_vehicle() {
            return InteractionResult::Pass;
        }
        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };
        let Some(minecart) = world.get_entity_by_id(self.id()) else {
            return InteractionResult::Pass;
        };
        if player.start_riding(&minecart) {
            InteractionResult::SuccessServer
        } else {
            InteractionResult::Consume
        }
    }

    /// A powered activator rail ejects the passengers and rocks the cart.
    fn activate_minecart(&self, _world: &World, _pos: BlockPos, powered: bool) {
        if !powered {
            return;
        }
        if self.is_vehicle() {
            self.eject_passengers();
        }
        let vehicle = self.vehicle_entity();
        if vehicle.hurt_time() == 0 {
            vehicle.set_hurt_dir(-vehicle.hurt_dir());
            vehicle.set_hurt_time(HURT_DURATION);
            vehicle.set_damage(ACTIVATOR_RAIL_DAMAGE);
            self.mark_hurt();
        }
    }
}

#[cfg(test)]
#[path = "tests/minecart.rs"]
mod tests;
