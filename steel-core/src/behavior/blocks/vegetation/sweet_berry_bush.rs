use std::sync::Arc;

use glam::DVec3;
use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties},
    item_stack::ItemStack,
    items::item::BlockHitResult,
    loot_table::LootContext,
    sound_events, vanilla_damage_types, vanilla_entities, vanilla_items,
    vanilla_loot_tables::{self},
};
use steel_utils::{
    BlockPos, BlockStateId,
    types::{InteractionHand, UpdateFlags},
};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess,
        blocks::vegetation::{
            Vegetation,
            bonemealable::Bonemealable,
            vegetation_block::{survival_update_shape, vegetation_can_survive},
        },
    },
    entity::{Entity, InsideBlockEffectCollector, damage::DamageSource},
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World},
};

const DAMAGE_MOVEMENT_THRESHOLD: f64 = 0.003;

/// Behavior for Sweet Berry Bushes
#[block_behavior]
pub struct SweetBerryBushBlock {
    block: BlockRef,
}

impl SweetBerryBushBlock {
    /// Creates a new Sweet Berry Bush Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SweetBerryBushBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(context.place_pos.below()),
            context.world,
            context.place_pos.below(),
        ) {
            Some(
                self.block
                    .default_state()
                    .set_value(&BlockStateProperties::AGE_3, 0),
            )
        } else {
            None
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        survival_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        state.get_value(&BlockStateProperties::AGE_3) < 3
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let age = state.get_value(&BlockStateProperties::AGE_3);
        if age >= 3 || rand::random_range(0..5) != 0 || world.raw_brightness(pos.above(), 0) < 9 {
            return;
        }
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::AGE_3, age + 1),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if !Self::applies_contact_effects(entity) {
            return;
        }

        entity.make_stuck_in_block(state, DVec3::new(0.8, 0.75, 0.8));
        Self::apply_contact_damage(world, state, entity);
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let is_bone_meal = inv.with_item(|item_stack| item_stack.is(&vanilla_items::BONE_MEAL));
        let age = state.get_value(&BlockStateProperties::AGE_3);
        if age != 3 && is_bone_meal {
            InteractionResult::Pass
        } else {
            InteractionResult::TryEmptyHandInteraction
        }
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let age = state.get_value(&BlockStateProperties::AGE_3);
        if age <= 1 {
            return InteractionResult::Pass;
        }
        let mut rng = rand::rng();
        let mut ctx = LootContext::new(&mut rng).with_block_state(state);

        let items = vanilla_loot_tables::HARVEST_SWEET_BERRY_BUSH.get_random_items(&mut ctx);
        for item in items {
            world.drop_item_stack(pos, item);
        }

        world.play_block_sound(
            &sound_events::BLOCK_SWEET_BERRY_BUSH_PICK_BERRIES,
            pos,
            1.0,
            0.8 + rng.random::<f32>() * 0.4,
            Some(player.id()),
        );

        let new_state = state.set_value(&BlockStateProperties::AGE_3, 1);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);

        InteractionResult::Success
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::SWEET_BERRIES))
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl SweetBerryBushBlock {
    fn applies_contact_effects(entity: &dyn Entity) -> bool {
        entity.is_living_entity()
            && entity.entity_type() != &vanilla_entities::FOX
            && entity.entity_type() != &vanilla_entities::BEE
    }

    fn apply_contact_damage(world: &World, state: BlockStateId, entity: &dyn Entity) {
        if state.get_value(&BlockStateProperties::AGE_3) == 0 {
            return;
        }

        let movement = if entity.uses_client_movement_packets() {
            entity.known_movement()
        } else {
            entity.old_position() - entity.position()
        };

        if movement.x.mul_add(movement.x, movement.z * movement.z) > 0.0
            && (movement.x.abs() >= DAMAGE_MOVEMENT_THRESHOLD
                || movement.z.abs() >= DAMAGE_MOVEMENT_THRESHOLD)
        {
            entity.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::SWEET_BERRY_BUSH),
                1.0,
            );
        }
    }
}

impl Bonemealable for SweetBerryBushBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        state.get_value(&BlockStateProperties::AGE_3) < 3
            && world.get_block_state(pos.above()).is_air()
            && !world.is_outside_build_height(pos.above().y())
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        let new_age = (state.get_value(&BlockStateProperties::AGE_3) + 1).min(3);
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::AGE_3, new_age),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}

impl Vegetation for SweetBerryBushBlock {}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::{
        entity_type::{EntityDimensions, EntityTypeRef},
        test_support::init_test_registry,
        vanilla_blocks,
    };
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::EntityBase;
    use crate::test_support::test_world;

    struct TestEntity {
        base: EntityBase,
        entity_type: EntityTypeRef,
        is_living: bool,
        uses_client_movement_packets: bool,
        known_movement: DVec3,
        damage: SyncMutex<Vec<(String, f32)>>,
    }

    impl TestEntity {
        fn living(entity_type: EntityTypeRef) -> Self {
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    EntityDimensions::new(0.6, 1.8, 1.62),
                    Weak::<World>::new(),
                ),
                entity_type,
                is_living: true,
                uses_client_movement_packets: false,
                known_movement: DVec3::ZERO,
                damage: SyncMutex::new(Vec::new()),
            }
        }

        fn with_position(self, position: DVec3) -> Self {
            self.base.set_position_local(position);
            self
        }

        fn with_old_position(self, old_position: DVec3) -> Self {
            self.base.set_old_position(old_position);
            self
        }

        fn with_client_movement(mut self, movement: DVec3) -> Self {
            self.uses_client_movement_packets = true;
            self.known_movement = movement;
            self
        }

        fn non_living(mut self) -> Self {
            self.is_living = false;
            self
        }

        fn damage_events(&self) -> Vec<(String, f32)> {
            self.damage.lock().clone()
        }
    }

    crate::entity::impl_test_downcast_type!(TestEntity);

    impl Entity for TestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn is_living_entity(&self) -> bool {
            self.is_living
        }

        fn uses_client_movement_packets(&self) -> bool {
            self.uses_client_movement_packets
        }

        fn known_movement(&self) -> DVec3 {
            self.known_movement
        }

        fn hurt(&self, _world: &World, source: &DamageSource, amount: f32) -> bool {
            self.damage
                .lock()
                .push((source.damage_type.key.path.as_ref().to_owned(), amount));
            true
        }
    }

    fn state_with_age(age: u8) -> BlockStateId {
        init_test_registry();
        vanilla_blocks::SWEET_BERRY_BUSH
            .default_state()
            .set_value(&BlockStateProperties::AGE_3, age)
    }

    #[test]
    fn contact_damage_uses_old_position_for_server_authored_entities() {
        let entity = TestEntity::living(&vanilla_entities::PLAYER)
            .with_position(DVec3::new(0.0, 0.0, 0.0))
            .with_old_position(DVec3::new(0.004, 0.0, 0.0));

        SweetBerryBushBlock::apply_contact_damage(test_world(), state_with_age(1), &entity);

        assert_eq!(
            entity.damage_events(),
            vec![("sweet_berry_bush".to_owned(), 1.0)]
        );
    }

    #[test]
    fn contact_damage_uses_known_movement_for_client_authored_entities() {
        let entity = TestEntity::living(&vanilla_entities::PLAYER)
            .with_position(DVec3::ZERO)
            .with_old_position(DVec3::ZERO)
            .with_client_movement(DVec3::new(0.0, 0.0, 0.004));

        SweetBerryBushBlock::apply_contact_damage(test_world(), state_with_age(1), &entity);

        assert_eq!(
            entity.damage_events(),
            vec![("sweet_berry_bush".to_owned(), 1.0)]
        );
    }

    #[test]
    fn contact_damage_is_age_gated() {
        let entity = TestEntity::living(&vanilla_entities::PLAYER)
            .with_position(DVec3::ZERO)
            .with_old_position(DVec3::new(0.004, 0.0, 0.0));

        SweetBerryBushBlock::apply_contact_damage(test_world(), state_with_age(0), &entity);

        assert!(entity.damage_events().is_empty());
    }

    #[test]
    fn contact_damage_requires_threshold_movement() {
        let entity = TestEntity::living(&vanilla_entities::PLAYER)
            .with_position(DVec3::ZERO)
            .with_old_position(DVec3::new(0.002_9, 0.0, 0.002_9));

        SweetBerryBushBlock::apply_contact_damage(test_world(), state_with_age(1), &entity);

        assert!(entity.damage_events().is_empty());
    }

    #[test]
    fn foxes_bees_and_non_living_entities_are_immune_to_sweet_berry_bush_effects() {
        let fox = TestEntity::living(&vanilla_entities::FOX);
        let bee = TestEntity::living(&vanilla_entities::BEE);
        let item = TestEntity::living(&vanilla_entities::ITEM).non_living();

        assert!(!SweetBerryBushBlock::applies_contact_effects(&fox));
        assert!(!SweetBerryBushBlock::applies_contact_effects(&bee));
        assert!(!SweetBerryBushBlock::applies_contact_effects(&item));
    }
}
