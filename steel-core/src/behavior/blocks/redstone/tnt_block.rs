//! TNT block behavior and priming entry points.

use std::borrow::Cow;
use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_game_rules::TNT_EXPLODES;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_entities, vanilla_game_events, vanilla_items,
};
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId};
use text_components::TextComponent;
use text_components::translation::TranslatedMessage;

use crate::behavior::{
    BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
};
use crate::entity::projectile::Projectile;
use crate::entity::{Entity, SharedEntity, entities::PrimedTntEntity, next_entity_id};
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{ClipHitResult, Explosion, SignalGetter as _, World};

/// Vanilla TNT block behavior.
#[block_behavior]
pub struct TntBlock {
    block: BlockRef,
}

impl TntBlock {
    /// Creates TNT block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn primed_entity(
        world: &Arc<World>,
        pos: BlockPos,
        owner: Option<&dyn Entity>,
    ) -> Arc<PrimedTntEntity> {
        Arc::new(PrimedTntEntity::primed(
            &vanilla_entities::TNT,
            next_entity_id(),
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()),
                f64::from(pos.z()) + 0.5,
            ),
            world,
            owner,
        ))
    }

    fn try_add_primed(world: &Arc<World>, entity: Arc<PrimedTntEntity>) -> bool {
        let entity: SharedEntity = entity;
        if let Err(error) = world.try_add_entity(entity) {
            log::debug!("failed to add primed TNT: {error}");
            return false;
        }
        true
    }

    /// Primes TNT at `pos`, returning whether a primed entity was created.
    ///
    /// This is crate-visible so shared fire burn ticking can call the same entry point when that
    /// foundation is implemented.
    pub(crate) fn prime(world: &Arc<World>, pos: BlockPos, owner: Option<&dyn Entity>) -> bool {
        if !world.get_game_rule(&TNT_EXPLODES) {
            return false;
        }

        let entity = Self::primed_entity(world, pos, owner);
        let position = entity.position();
        if !Self::try_add_primed(world, entity) {
            return false;
        }
        world.play_sound_at(
            &sound_events::ENTITY_TNT_PRIMED,
            SoundSource::Blocks,
            position,
            1.0,
            1.0,
            None,
        );
        world.game_event(
            &vanilla_game_events::PRIME_FUSE,
            pos,
            &GameEventContext::new(owner, None),
        );
        true
    }

    fn send_disabled_message(player: &Player) {
        player.send_overlay_message(&TextComponent::translated(TranslatedMessage {
            key: Cow::Borrowed("block.minecraft.tnt.disabled"),
            fallback: None,
            args: None,
        }));
    }
}

impl BlockBehavior for TntBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != state.get_block()
            && world.has_neighbor_signal(pos)
            && Self::prime(world, pos, None)
        {
            world.remove_block(pos, false);
        }
    }

    fn handle_neighbor_changed(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if world.has_neighbor_signal(pos) && Self::prime(world, pos, None) {
            world.remove_block(pos, false);
        }
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        if !player.has_infinite_materials() && state.get_value(&BlockStateProperties::UNSTABLE) {
            Self::prime(world, pos, None);
        }
        state
    }

    fn was_exploded(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        explosion: &dyn Explosion,
    ) {
        if !world.get_game_rule(&TNT_EXPLODES) {
            return;
        }

        let entity = Self::primed_entity(world, pos, explosion.indirect_source_entity());
        entity.set_fuse(PrimedTntEntity::get_random_short_fuse(world, entity.fuse()));
        Self::try_add_primed(world, entity);
    }

    fn use_item_on(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let (is_flint_and_steel, is_fire_charge) = inv.with_item(|item| {
            (
                item.is(&vanilla_items::FLINT_AND_STEEL),
                item.is(&vanilla_items::FIRE_CHARGE),
            )
        });
        if !is_flint_and_steel && !is_fire_charge {
            return InteractionResult::TryEmptyHandInteraction;
        }

        if !Self::prime(world, pos, Some(player)) {
            if !world.get_game_rule(&TNT_EXPLODES) {
                Self::send_disabled_message(player);
                return InteractionResult::Pass;
            }
            return InteractionResult::Success;
        }

        world.set_block(
            pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL_IMMEDIATE,
        );
        let has_infinite_materials = player.has_infinite_materials();
        inv.with_item(|item| {
            if is_flint_and_steel {
                item.hurt_and_break(1, has_infinite_materials);
            } else if !has_infinite_materials {
                item.shrink(1);
            }
        });
        // TODO: Award the ITEM_USED statistic once Steel has shared statistics support.
        InteractionResult::Success
    }

    fn on_projectile_hit(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        if !projectile.is_on_fire() || !projectile.projectile_may_interact(world, hit.block_pos) {
            return;
        }

        let owner = projectile.get_owner();
        let living_owner = owner.as_deref().filter(|owner| owner.is_living_entity());
        if Self::prime(world, hit.block_pos, living_owner) {
            world.remove_block(hit.block_pos, false);
        }
    }

    fn drop_from_explosion(&self, _explosion: &dyn Explosion) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::init_vanilla_registry;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::vanilla_game_rules::{MOB_GRIEFING, TNT_EXPLODES};
    use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_items};
    use steel_utils::types::InteractionHand;
    use steel_utils::{ChunkPos, Direction, Downcast as _, WorldAabb};
    use uuid::Uuid;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::entity::Entity;
    use crate::entity::entities::{FireworkRocketEntity, PigEntity, PrimedTntEntity};
    use crate::player::ResetReason;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use crate::world::{ExplosionInteraction, ExplosionOptions};

    fn setup_world(key: &'static str) -> (Arc<World>, BlockPos) {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        (world, pos)
    }

    fn primed_tnt_entities(world: &World, pos: BlockPos) -> Vec<SharedEntity> {
        let bounds = WorldAabb::new(
            f64::from(pos.x()),
            f64::from(pos.y()) - 1.0,
            f64::from(pos.z()),
            f64::from(pos.x()) + 1.0,
            f64::from(pos.y()) + 2.0,
            f64::from(pos.z()) + 1.0,
        );
        world
            .get_entities_in_aabb(&bounds)
            .into_iter()
            .filter(|entity| entity.downcast_ref::<PrimedTntEntity>().is_some())
            .collect()
    }

    fn hit_result(pos: BlockPos) -> BlockHitResult {
        BlockHitResult {
            location: DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        }
    }

    #[test]
    fn powered_placement_primes_and_removes_tnt() {
        let (world, pos) = setup_world("tnt_redstone_placement");
        assert!(world.set_block(
            pos.west(),
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        assert!(world.get_block_state(pos).is_air());
        let primed = primed_tnt_entities(&world, pos);
        assert_eq!(primed.len(), 1);
        assert_eq!(
            primed[0]
                .downcast_ref::<PrimedTntEntity>()
                .map(PrimedTntEntity::fuse),
            Some(80)
        );
    }

    #[test]
    fn neighbor_power_primes_an_existing_tnt_block() {
        let (world, pos) = setup_world("tnt_redstone_neighbor");
        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        assert!(world.set_block(
            pos.west(),
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        assert!(world.get_block_state(pos).is_air());
        assert_eq!(primed_tnt_entities(&world, pos).len(), 1);
    }

    #[test]
    fn unstable_survival_break_primes_tnt_without_a_player_owner() {
        let (world, pos) = setup_world("tnt_unstable_survival_break");
        let state = vanilla_blocks::TNT
            .default_state()
            .set_value(&BlockStateProperties::UNSTABLE, true);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let player = TestPlayerBuilder::new(
            Arc::clone(&world),
            Uuid::from_u128(0xB8EA),
            "Breaker",
            next_entity_id(),
        )
        .build();

        let returned_state = BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TNT)
            .player_will_destroy(state, &world, pos, &player);

        assert_eq!(returned_state, state);
        let primed = primed_tnt_entities(&world, pos);
        assert_eq!(primed.len(), 1);
        assert!(primed[0].explosion_indirect_source().is_none());
    }

    #[test]
    fn flint_and_steel_primes_tnt_damages_item_and_tracks_owner() {
        let (world, pos) = setup_world("tnt_flint_and_steel");
        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let player = TestPlayerBuilder::new(
            Arc::clone(&world),
            Uuid::from_u128(0x7117),
            "Igniter",
            next_entity_id(),
        )
        .build();
        player.base().set_position_local(DVec3::new(8.5, 64.0, 8.5));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FLINT_AND_STEEL));
        let mut inventory =
            InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);

        let result = BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TNT)
            .use_item_on(
                world.get_block_state(pos),
                &world,
                pos,
                &player,
                InteractionHand::MainHand,
                &hit_result(pos),
                &mut inventory,
            );

        assert_eq!(result, InteractionResult::Success);
        assert!(world.get_block_state(pos).is_air());
        assert_eq!(
            player
                .inventory
                .lock()
                .get_selected_item()
                .get_damage_value(),
            1
        );
        let primed = primed_tnt_entities(&world, pos);
        assert_eq!(primed.len(), 1);
        assert_eq!(
            primed[0]
                .explosion_indirect_source()
                .map(|owner| owner.id()),
            Some(player.id())
        );
    }

    #[test]
    fn fire_charge_primes_tnt_and_consumes_one_item() {
        let (world, pos) = setup_world("tnt_fire_charge");
        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let player = TestPlayerBuilder::new(
            Arc::clone(&world),
            Uuid::from_u128(0xF1),
            "Charger",
            next_entity_id(),
        )
        .build();
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::with_count(&vanilla_items::FIRE_CHARGE, 2));
        let mut inventory =
            InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);

        let result = BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TNT)
            .use_item_on(
                world.get_block_state(pos),
                &world,
                pos,
                &player,
                InteractionHand::MainHand,
                &hit_result(pos),
                &mut inventory,
            );

        assert_eq!(result, InteractionResult::Success);
        assert_eq!(player.inventory.lock().get_selected_item().count(), 1);
        assert_eq!(primed_tnt_entities(&world, pos).len(), 1);
    }

    #[test]
    fn disabled_tnt_gamerule_preserves_block_and_ignition_item() {
        let (world, pos) = setup_world("tnt_disabled_gamerule");
        assert!(world.set_game_rule(&TNT_EXPLODES, false));
        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let player = TestPlayerBuilder::new(
            Arc::clone(&world),
            Uuid::from_u128(0xD15A_B1ED),
            "Disabled",
            next_entity_id(),
        )
        .build();
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FLINT_AND_STEEL));
        let mut inventory =
            InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);

        let result = BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TNT)
            .use_item_on(
                world.get_block_state(pos),
                &world,
                pos,
                &player,
                InteractionHand::MainHand,
                &hit_result(pos),
                &mut inventory,
            );

        assert_eq!(result, InteractionResult::Pass);
        assert_eq!(world.get_block_state(pos).get_block(), &vanilla_blocks::TNT);
        assert_eq!(
            player
                .inventory
                .lock()
                .get_selected_item()
                .get_damage_value(),
            0
        );
        assert!(primed_tnt_entities(&world, pos).is_empty());
    }

    #[test]
    fn explosion_replaces_tnt_with_a_short_fuse_entity() {
        let (world, pos) = setup_world("tnt_chain_reaction");
        assert!(world.set_block(
            pos,
            vanilla_blocks::TNT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let owner = TestPlayerBuilder::new(
            Arc::clone(&world),
            Uuid::from_u128(0xC4A1),
            "ChainOwner",
            next_entity_id(),
        )
        .build();
        owner.base().set_position_local(DVec3::new(12.5, 64.0, 8.5));
        assert!(world.add_player(Arc::clone(&owner), ResetReason::InitialJoin));
        let mut options = ExplosionOptions::new(
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            4.0,
            ExplosionInteraction::Tnt,
        );
        options.source = Some(owner.as_ref());

        world.explode(options);

        assert!(world.get_block_state(pos).is_air());
        let primed = primed_tnt_entities(&world, pos);
        assert_eq!(primed.len(), 1);
        let fuse = primed[0]
            .downcast_ref::<PrimedTntEntity>()
            .map(PrimedTntEntity::fuse)
            .expect("spawned entity should remain primed TNT");
        assert!((10..30).contains(&fuse));
        assert_eq!(
            primed[0]
                .explosion_indirect_source()
                .map(|source| source.id()),
            Some(owner.id())
        );
    }

    #[test]
    fn fuse_tick_detonates_primed_tnt() {
        let (world, pos) = setup_world("primed_tnt_detonation");
        assert!(world.set_block(
            pos,
            vanilla_blocks::GLASS.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let entity = Arc::new(PrimedTntEntity::new(
            &vanilla_entities::TNT,
            next_entity_id(),
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()),
                f64::from(pos.z()) + 0.5,
            ),
            Arc::downgrade(&world),
        ));
        entity.set_fuse(1);
        let shared: SharedEntity = entity.clone();
        world
            .try_add_entity(shared)
            .expect("primed TNT should enter the loaded test chunk");

        entity.tick();

        assert!(entity.is_removed());
        assert!(world.get_block_state(pos).is_air());
    }

    #[test]
    fn burning_projectile_primes_tnt() {
        let (world, pos) = setup_world("tnt_burning_projectile");
        let state = vanilla_blocks::TNT.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let projectile = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            next_entity_id(),
            DVec3::new(8.5, 64.5, 8.5),
            Arc::downgrade(&world),
        );
        projectile.set_remaining_fire_ticks(20);
        let block_hit = hit_result(pos);
        let clip_hit = ClipHitResult {
            location: block_hit.location,
            direction: block_hit.direction,
            block_pos: block_hit.block_pos,
            miss: block_hit.miss,
            inside: block_hit.inside,
            world_border_hit: block_hit.world_border_hit,
        };

        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TNT)
            .on_projectile_hit(state, &world, &clip_hit, &projectile);

        assert!(world.get_block_state(pos).is_air());
        assert_eq!(primed_tnt_entities(&world, pos).len(), 1);
    }

    #[test]
    fn burning_projectile_preserves_its_living_owner() {
        let (world, pos) = setup_world("tnt_burning_projectile_owner");
        let state = vanilla_blocks::TNT.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let player = TestPlayerBuilder::new(
            Arc::clone(&world),
            Uuid::from_u128(0xA220),
            "Archer",
            next_entity_id(),
        )
        .build();
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let projectile = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            next_entity_id(),
            DVec3::new(8.5, 64.5, 8.5),
            Arc::downgrade(&world),
        );
        projectile.set_owner_uuid(Some(player.uuid()));
        projectile.set_remaining_fire_ticks(20);
        let block_hit = hit_result(pos);
        let clip_hit = ClipHitResult {
            location: block_hit.location,
            direction: block_hit.direction,
            block_pos: block_hit.block_pos,
            miss: block_hit.miss,
            inside: block_hit.inside,
            world_border_hit: block_hit.world_border_hit,
        };

        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TNT)
            .on_projectile_hit(state, &world, &clip_hit, &projectile);

        let primed = primed_tnt_entities(&world, pos);
        assert_eq!(primed.len(), 1);
        assert_eq!(
            primed[0]
                .explosion_indirect_source()
                .map(|owner| owner.id()),
            Some(player.id())
        );
    }

    #[test]
    fn burning_mob_owned_projectile_respects_mob_griefing() {
        let (world, pos) = setup_world("tnt_projectile_mob_griefing");
        assert!(world.set_game_rule(&MOB_GRIEFING, false));
        let state = vanilla_blocks::TNT.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let projectile = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            next_entity_id(),
            DVec3::new(8.5, 64.5, 8.5),
            Arc::downgrade(&world),
        );
        let mob = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
        ));
        let shared_mob: SharedEntity = mob.clone();
        assert!(world.try_add_entity(shared_mob).is_ok());
        projectile.set_owner_uuid(Some(mob.uuid()));
        projectile.set_remaining_fire_ticks(20);
        let block_hit = hit_result(pos);
        let clip_hit = ClipHitResult {
            location: block_hit.location,
            direction: block_hit.direction,
            block_pos: block_hit.block_pos,
            miss: block_hit.miss,
            inside: block_hit.inside,
            world_border_hit: block_hit.world_border_hit,
        };

        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TNT)
            .on_projectile_hit(state, &world, &clip_hit, &projectile);

        assert_eq!(world.get_block_state(pos), state);
        assert!(primed_tnt_entities(&world, pos).is_empty());
    }
}
