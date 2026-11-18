use glam::DVec3;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::DefaultDispenseBehavior;
use super::default::nozzle_position;
use super::{DispenseItemBehavior, DispenseResult};
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::enchantment_helper;
use crate::entity::{ENTITIES, Entity, next_entity_id};
use crate::world::World;

const PROJECTILE_Y_OFFSET: f64 = 0.1;

pub struct ProjectileDispenseBehavior {
    entity_type: EntityTypeRef,
    power: f32,
    uncertainty: f32,
    item_setter: Option<fn(&dyn Entity, &ItemStack)>,
}

impl ProjectileDispenseBehavior {
    pub fn new(entity_type: EntityTypeRef, power: f32, uncertainty: f32) -> Self {
        Self {
            entity_type,
            power,
            uncertainty,
            item_setter: None,
        }
    }

    #[must_use]
    pub const fn with_item_setter(mut self, item_setter: fn(&dyn Entity, &ItemStack)) -> Self {
        self.item_setter = Some(item_setter);
        self
    }
}

impl DispenseItemBehavior for ProjectileDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        let facing = state.get_value(FACING);
        let offset = facing.offset_vec();

        let spawn_pos = nozzle_position(pos, facing) + DVec3::Y * PROJECTILE_Y_OFFSET;

        let id = next_entity_id();
        let Some(entity) = ENTITIES.create(self.entity_type, id, spawn_pos, Arc::downgrade(world))
        else {
            return DefaultDispenseBehavior.dispense(world, pos, state, item);
        };

        if let Some(item_setter) = self.item_setter {
            item_setter(entity.as_ref(), item);
        }

        if let Some(projectile) = entity.as_projectile() {
            projectile.shoot(offset.as_dvec3(), self.power, self.uncertainty);
        }

        if world.try_add_entity(Arc::clone(&entity)).is_ok() {
            enchantment_helper::on_projectile_spawned(world, item, entity.as_ref(), None);
        }

        item.shrink(1);
        DispenseResult::Projectile
    }
}
