use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::{level_events, vanilla_entities};
use steel_utils::{BlockPos, BlockStateId, Direction, Downcast as _};

use super::DefaultDispenseBehavior;
use super::default::nozzle_position;
use super::{DispenseItemBehavior, DispenseResult};
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::enchantment_helper;
use crate::entity::entities::FireworkRocketEntity;
use crate::entity::{ENTITIES, Entity, next_entity_id};
use crate::world::World;

const PROJECTILE_Y_OFFSET: f64 = 0.1;
const FIREWORK_SPAWN_DISTANCE: f64 = 0.500_009_999_999_747_4;
const FIREWORK_POWER: f32 = 0.5;
const FIREWORK_SPREAD: f32 = 1.0;

pub struct ProjectileDispenseBehavior {
    entity_type: EntityTypeRef,
    power: f32,
    uncertainty: f32,
    item_setter: Option<fn(&dyn Entity, &ItemStack)>,
    spawn_position: fn(BlockPos, Direction) -> DVec3,
    sound: i32,
}

impl ProjectileDispenseBehavior {
    pub fn new(entity_type: EntityTypeRef, power: f32, uncertainty: f32) -> Self {
        Self {
            entity_type,
            power,
            uncertainty,
            item_setter: None,
            spawn_position: |pos, facing| {
                nozzle_position(pos, facing) + DVec3::Y * PROJECTILE_Y_OFFSET
            },
            sound: level_events::SOUND_DISPENSER_PROJECTILE_LAUNCH,
        }
    }

    pub(super) fn firework() -> Self {
        Self {
            spawn_position: |pos, facing| {
                pos.get_center() + facing.offset_vec().as_dvec3() * FIREWORK_SPAWN_DISTANCE
            },
            sound: level_events::SOUND_FIREWORK_SHOOT,
            ..Self::new(
                &vanilla_entities::FIREWORK_ROCKET,
                FIREWORK_POWER,
                FIREWORK_SPREAD,
            )
            .with_item_setter(|entity, item| {
                if let Some(rocket) = entity.downcast_ref::<FireworkRocketEntity>() {
                    rocket.initialize_launch(item.copy_with_count(1));
                    rocket.set_shot_at_angle(true);
                }
            })
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

        let spawn_pos = (self.spawn_position)(pos, facing);

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
        DispenseResult::Projectile(self.sound)
    }
}
