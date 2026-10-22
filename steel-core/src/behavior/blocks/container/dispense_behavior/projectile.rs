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
use crate::entity::{ENTITIES, Entity, next_entity_id};
use crate::world::World;

pub struct ProjectileDispenseBehavior {
    entity_type: EntityTypeRef,
    power: f32,
    uncertainty: f32,
    on_spawn: Option<fn(&dyn Entity, &ItemStack)>,
}

impl ProjectileDispenseBehavior {
    pub fn new(entity_type: EntityTypeRef, power: f32, uncertainty: f32) -> Self {
        Self {
            entity_type,
            power,
            uncertainty,
            on_spawn: None,
        }
    }

    #[must_use]
    pub const fn with_item_setter(mut self, on_spawn: fn(&dyn Entity, &ItemStack)) -> Self {
        self.on_spawn = Some(on_spawn);
        self
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
            f64::from(pos.x()) + 0.5 + (f64::from(offset.x) * 0.7),
            f64::from(pos.y()) + 0.5 + (f64::from(offset.y) * 0.7),
            f64::from(pos.z()) + 0.5 + (f64::from(offset.z) * 0.7),
        );

        let id = next_entity_id();
        let Some(entity) = ENTITIES.create(self.entity_type, id, spawn_pos, Arc::downgrade(world))
        else {
            return DefaultDispenseBehavior.dispense(world, pos, state, item);
        };

        if let Some(on_spawn) = self.on_spawn {
            on_spawn(entity.as_ref(), &item);
        }

        if let Some(projectile) = entity.as_projectile() {
            let direction = DVec3::new(
                f64::from(offset.x),
                f64::from(offset.y),
                f64::from(offset.z),
            )
            .normalize();
            // Default Minecraft uses power=1.1, uncertainty=6.0 for eggs/snowballs
            projectile.shoot(direction, self.power, self.uncertainty);
        }

        let _ = world.try_add_entity(entity);
        world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);

        ItemStack::empty()
    }
}
