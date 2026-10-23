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
use crate::entity::{ENTITIES, next_entity_id};
use crate::world::World;

pub struct ArrowDispenseBehavior {
    entity_type: EntityTypeRef,
}

impl ArrowDispenseBehavior {
    pub const fn new(entity_type: EntityTypeRef) -> Self {
        Self { entity_type }
    }
}

impl DispenseItemBehavior for ArrowDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        mut item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let offset = facing.offset_vec();

        let spawn_pos = DVec3::new(
            f64::from(pos.x()) + 0.5 + (f64::from(offset.x) * 0.7),
            f64::from(pos.y()) + 0.5 + (f64::from(offset.y) * 0.7),
            f64::from(pos.z()) + 0.5 + (f64::from(offset.z) * 0.7),
        );

        let id = next_entity_id();
        let Some(entity) = ENTITIES.create(self.entity_type, id, spawn_pos, Arc::downgrade(world))
        else {
            return DefaultDispenseBehavior.dispense(world, pos, state, item);
        };

        if let Some(projectile) = entity.as_projectile() {
            let direction = DVec3::new(
                f64::from(offset.x),
                f64::from(offset.y),
                f64::from(offset.z),
            )
            .normalize();
            // Arrow specific defaults
            projectile.shoot(direction, 1.1, 6.0);

            // Note: In vanilla, arrows shot from a dispenser can be picked up.
            // That requires setting the pickup status on the arrow entity.
            // Since we don't have the explicit Arrow entity cast here, we rely on the
            // base spawn to set it up, but usually this is done via AbstractArrow entity.
            // (Assuming SteelMC's default spawn handles it or we'll need an entity extension trait).
        }

        let _ = world.try_add_entity(entity);
        world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);
        item.shrink(1);

        item
    }
}
