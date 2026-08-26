use glam::DVec3;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::DefaultDispenseBehavior;
use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::World;

pub struct ProjectileDispenseBehavior {
    entity_type: EntityTypeRef,
    power: f32,
    uncertainty: f32,
}

impl ProjectileDispenseBehavior {
    pub fn new(entity_type: EntityTypeRef, power: f32, uncertainty: f32) -> Self {
        Self {
            entity_type,
            power,
            uncertainty,
        }
    }
}

impl DispenseItemBehavior for ProjectileDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let offset = facing.offset_vec();

        let spawn_pos = DVec3::new(
            pos.x() as f64 + 0.5 + (offset.x as f64 * 0.7),
            pos.y() as f64 + 0.5 + (offset.y as f64 * 0.7),
            pos.z() as f64 + 0.5 + (offset.z as f64 * 0.7),
        );

        let id = crate::entity::next_entity_id();
        let Some(entity) =
            crate::entity::ENTITIES.create(self.entity_type, id, spawn_pos, Arc::downgrade(world))
        else {
            return DefaultDispenseBehavior.dispense(world, pos, state, item);
        };

        if let Some(projectile) = entity.as_projectile() {
            let direction =
                DVec3::new(offset.x as f64, offset.y as f64, offset.z as f64).normalize();
            // Default Minecraft uses power=1.1, uncertainty=6.0 for eggs/snowballs
            projectile.shoot(direction, self.power, self.uncertainty);
        }

        let _ = world.try_add_entity(entity);
        world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);

        ItemStack::empty()
    }
}
