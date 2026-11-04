use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::properties::IntProperty;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, Direction},
};
use steel_registry::fluid::{FluidState, is_water_fluid};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::item::BlockHitResult;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_damage_types, vanilla_game_events, vanilla_items,
};
use steel_utils::{
    BlockPos, BlockStateId,
    types::{InteractionHand, UpdateFlags},
};

use crate::entity::damage::DamageSource;
use crate::entity::dismount_helper;
use crate::fluid::get_fluid_state;
use crate::{
    behavior::blocks::redstone::MAX_REDSTONE_SIGNAL,
    behavior::{BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess},
    entity::Entity,
    level_data::RespawnData,
    player::{Player, PlayerRespawnConfig},
    world::{
        DefaultExplosionDamageCalculator, Explosion, ExplosionBlockReader,
        ExplosionDamageCalculator, ExplosionInteraction, ExplosionOptions,
        ImmutableExplosionBlockCalculator, LevelReader, World, game_event::GameEventContext,
    },
};

const EXPLOSION_RADIUS: f32 = 5.0;

/// Gives the removed anchor's position water's resistance when the anchor was in water,
/// shielding blocks behind it from the blast.
struct RespawnAnchorExplosionDamageCalculator {
    anchor_pos: BlockPos,
    in_water: bool,
}

impl RespawnAnchorExplosionDamageCalculator {
    fn resistance_override(&self, pos: BlockPos) -> Option<f32> {
        (self.in_water && pos == self.anchor_pos)
            .then_some(vanilla_blocks::WATER.config.explosion_resistance)
    }
}

impl ExplosionDamageCalculator for RespawnAnchorExplosionDamageCalculator {
    fn block_explosion_resistance(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        self.resistance_override(pos).or_else(|| {
            DefaultExplosionDamageCalculator
                .block_explosion_resistance(explosion, world, pos, state, fluid)
        })
    }
}

impl ImmutableExplosionBlockCalculator for RespawnAnchorExplosionDamageCalculator {
    fn bounded_block_read_radius(&self) -> Option<u32> {
        Some(0)
    }

    fn can_cache_explosion_resistance(&self) -> bool {
        true
    }

    fn always_allows_block_explosion(&self) -> bool {
        true
    }

    fn explosion_resistance(
        &self,
        reader: &dyn ExplosionBlockReader,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        self.resistance_override(pos).or_else(|| {
            DefaultExplosionDamageCalculator.explosion_resistance(reader, pos, state, fluid)
        })
    }
}

/// Vanilla respawn anchor
#[block_behavior]
pub struct RespawnAnchorBlock {
    block: BlockRef,
}
const CHARGES: &IntProperty = &BlockStateProperties::RESPAWN_ANCHOR_CHARGES;
impl RespawnAnchorBlock {
    const MAX_CHARGES: u8 = 4;

    /// New respawn anchor behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    pub(crate) fn can_use_for_respawn(
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        forced: bool,
    ) -> bool {
        state.get_block() == &vanilla_blocks::RESPAWN_ANCHOR
            && (forced || state.get_value(CHARGES) > 0)
            && Self::can_set_spawn(world, pos)
    }

    #[must_use]
    pub(crate) const fn can_set_spawn(world: &World, _pos: BlockPos) -> bool {
        world.dimension_type.respawn_anchor_works
    }

    pub(crate) fn find_standup_position(
        world: &Arc<World>,
        entity: &dyn Entity,
        pos: BlockPos,
    ) -> Option<DVec3> {
        for (candidate, check_dangerous) in Self::standup_search_candidates(pos) {
            if let Some(position) = dismount_helper::find_safe_dismount_location(
                world,
                entity,
                candidate,
                check_dangerous,
            ) {
                return Some(position);
            }
        }

        None
    }

    fn state_after_charge_consumed(state: BlockStateId) -> Option<BlockStateId> {
        if state.get_block() != &vanilla_blocks::RESPAWN_ANCHOR {
            return None;
        }
        let charges = state.get_value(CHARGES);
        (charges > 0).then(|| state.set_value(CHARGES, charges - 1))
    }

    #[must_use]
    pub(crate) fn consume_charge(world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> bool {
        if world.get_block_state(pos) != state {
            return false;
        }
        let Some(state) = Self::state_after_charge_consumed(state) else {
            return false;
        };
        world.set_block(pos, state, UpdateFlags::UPDATE_ALL)
    }

    #[must_use]
    pub(crate) const fn should_consume_charge_after_respawn(
        forced: bool,
        consume_spawn_block: bool,
        found_standup_position: bool,
    ) -> bool {
        !forced && consume_spawn_block && found_standup_position
    }

    fn standup_candidates(pos: BlockPos) -> impl Iterator<Item = BlockPos> {
        [pos, pos.below(), pos.above()]
            .into_iter()
            .flat_map(Self::horizontal_standup_candidates)
            .chain([pos.above()])
    }

    fn standup_search_candidates(pos: BlockPos) -> impl Iterator<Item = (BlockPos, bool)> {
        [true, false].into_iter().flat_map(move |check_dangerous| {
            Self::standup_candidates(pos).map(move |candidate| (candidate, check_dangerous))
        })
    }

    const fn horizontal_standup_candidates(pos: BlockPos) -> [BlockPos; 8] {
        [
            pos.north(),
            pos.west(),
            pos.south(),
            pos.east(),
            pos.north().west(),
            pos.north().east(),
            pos.south().west(),
            pos.south().east(),
        ]
    }

    fn has_charge(state: BlockStateId) -> bool {
        state.get_value(CHARGES) > 0
    }

    fn can_be_charged(state: BlockStateId) -> bool {
        state.get_value(CHARGES) < Self::MAX_CHARGES
    }

    fn is_respawn_fuel(item_stack: &ItemStack) -> bool {
        item_stack.is(&vanilla_items::GLOWSTONE)
    }

    fn analog_output_signal(charges: u8) -> i32 {
        i32::from(charges) * MAX_REDSTONE_SIGNAL / i32::from(Self::MAX_CHARGES)
    }

    fn player_offhand_has_respawn_fuel(player: &Player) -> bool {
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::OffHand)
            .is(&vanilla_items::GLOWSTONE)
    }

    fn is_water_that_would_flow(world: &Arc<World>, pos: BlockPos) -> bool {
        let fluid = get_fluid_state(world, pos);
        if !is_water_fluid(fluid.fluid_id) {
            return false;
        }
        if fluid.is_source() {
            return true;
        }
        if fluid.amount < 2 {
            return false;
        }
        !is_water_fluid(get_fluid_state(world, pos.below()).fluid_id)
    }

    /// Checked after the anchor is removed, matching vanilla's order.
    fn is_in_water(world: &Arc<World>, pos: BlockPos) -> bool {
        Direction::HORIZONTAL
            .into_iter()
            .any(|direction| Self::is_water_that_would_flow(world, direction.relative(pos)))
            || is_water_fluid(get_fluid_state(world, pos.above()).fluid_id)
    }

    fn explode(world: &Arc<World>, pos: BlockPos) {
        world.remove_block(pos, false);
        let calculator = RespawnAnchorExplosionDamageCalculator {
            anchor_pos: pos,
            in_water: Self::is_in_water(world, pos),
        };
        let (x, y, z) = pos.get_center();
        let center = DVec3::new(x, y, z);
        let mut options =
            ExplosionOptions::new(center, EXPLOSION_RADIUS, ExplosionInteraction::Block);
        options.damage_source = Some(
            DamageSource::environment(&vanilla_damage_types::BAD_RESPAWN_POINT)
                .with_source_position(center),
        );
        options.damage_calculator = Some(&calculator);
        options.immutable_block_calculator = Some(&calculator);
        options.fire = true;
        world.explode(options);
    }

    fn charge(source: Option<&dyn Entity>, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let charges = state.get_value(CHARGES);
        let charged_state = state.set_value(CHARGES, charges + 1);
        world.set_block(pos, charged_state, UpdateFlags::UPDATE_ALL);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(source, Some(charged_state)),
        );
        world.play_sound(
            &sound_events::BLOCK_RESPAWN_ANCHOR_CHARGE,
            SoundSource::Blocks,
            pos,
            1.0,
            1.0,
            None,
        );
    }
}

impl BlockBehavior for RespawnAnchorBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let is_respawn_fuel = inv.with_item(|item_stack| Self::is_respawn_fuel(item_stack));
        if is_respawn_fuel && Self::can_be_charged(state) {
            Self::charge(Some(player), world, pos, state);
            let has_infinite_materials = player.has_infinite_materials();
            inv.with_item(|item_stack| {
                item_stack.consume_one(has_infinite_materials);
            });
            return InteractionResult::Success;
        }

        if hand == InteractionHand::MainHand
            && Self::can_be_charged(state)
            && Self::player_offhand_has_respawn_fuel(player)
        {
            return InteractionResult::Pass;
        }

        InteractionResult::TryEmptyHandInteraction
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
        if !Self::has_charge(state) {
            return InteractionResult::Pass;
        }

        if !Self::can_set_spawn(world, pos) {
            Self::explode(world, pos);
            return InteractionResult::SuccessServer;
        }

        let respawn_config = PlayerRespawnConfig {
            respawn_data: RespawnData::of(world.key.clone(), pos, 0.0, 0.0),
            forced: false,
        };
        let should_update_spawn = player.respawn_config().as_ref().is_none_or(|current| {
            current.respawn_data.global_pos != respawn_config.respawn_data.global_pos
        });

        if !should_update_spawn {
            return InteractionResult::Consume;
        }

        player.set_respawn_position(Some(respawn_config), true);
        world.play_sound(
            &sound_events::BLOCK_RESPAWN_ANCHOR_SET_SPAWN,
            SoundSource::Blocks,
            pos,
            1.0,
            1.0,
            None,
        );
        InteractionResult::SuccessServer
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        _world: &World,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        Self::analog_output_signal(state.get_value(CHARGES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::{BlockPos, ChunkPos};

    const ANCHOR_POS: BlockPos = BlockPos::new(8, 64, 8);
    const SET_FLAGS: UpdateFlags = UpdateFlags::UPDATE_NONE
        .union(UpdateFlags::UPDATE_KNOWN_SHAPE)
        .union(UpdateFlags::UPDATE_SKIP_ON_PLACE);

    fn anchor_world(key: &'static str) -> TestWorld {
        init_vanilla_registry();
        init_behaviors();
        let fixture = fresh_test_world(key);
        insert_ready_full_chunk(&fixture.world, ChunkPos::from_block_pos(ANCHOR_POS));
        fixture
    }

    fn water(level: u8) -> BlockStateId {
        vanilla_blocks::WATER
            .default_state()
            .set_value(&BlockStateProperties::LEVEL, level)
    }

    #[test]
    fn in_water_follows_vanilla_flow_rules() {
        let fixture = anchor_world("respawn_anchor_in_water");
        let world = &fixture.world;
        let side = ANCHOR_POS.north();
        let air = vanilla_blocks::AIR.default_state();
        let place = |pos, state| assert!(world.set_block(pos, state, SET_FLAGS));

        assert!(!RespawnAnchorBlock::is_in_water(world, ANCHOR_POS));

        // Flowing water with amount 1 cannot reach the anchor.
        place(side, water(7));
        assert!(!RespawnAnchorBlock::is_in_water(world, ANCHOR_POS));

        // Flowing water that would drain downward instead does not count either.
        place(side, water(6));
        place(side.below(), water(0));
        assert!(!RespawnAnchorBlock::is_in_water(world, ANCHOR_POS));
        place(side.below(), air);
        assert!(RespawnAnchorBlock::is_in_water(world, ANCHOR_POS));

        // Any water above counts, regardless of amount.
        place(side, air);
        place(ANCHOR_POS.above(), water(7));
        assert!(RespawnAnchorBlock::is_in_water(world, ANCHOR_POS));
    }

    #[test]
    fn water_shields_blocks_from_anchor_explosion() {
        for (in_water, key) in [
            (true, "respawn_anchor_explosion_in_water"),
            (false, "respawn_anchor_explosion_dry"),
        ] {
            let fixture = anchor_world(key);
            let world = &fixture.world;
            let glass_pos = ANCHOR_POS.north();
            let place = |pos, state| assert!(world.set_block(pos, state, SET_FLAGS));
            place(ANCHOR_POS, vanilla_blocks::RESPAWN_ANCHOR.default_state());
            place(glass_pos, vanilla_blocks::GLASS.default_state());
            if in_water {
                place(ANCHOR_POS.above(), water(0));
            }

            RespawnAnchorBlock::explode(world, ANCHOR_POS);

            assert_ne!(
                world.get_block_state(ANCHOR_POS).get_block(),
                &vanilla_blocks::RESPAWN_ANCHOR
            );
            assert_eq!(
                world.get_block_state(glass_pos).get_block() == &vanilla_blocks::GLASS,
                in_water,
            );
        }
    }

    #[test]
    fn standup_candidates_match_vanilla_order() {
        let origin = BlockPos::new(10, 64, -20);
        let candidates = RespawnAnchorBlock::standup_candidates(origin).collect::<Vec<_>>();

        assert_eq!(
            candidates,
            vec![
                origin.north(),
                origin.west(),
                origin.south(),
                origin.east(),
                origin.north().west(),
                origin.north().east(),
                origin.south().west(),
                origin.south().east(),
                origin.below().north(),
                origin.below().west(),
                origin.below().south(),
                origin.below().east(),
                origin.below().north().west(),
                origin.below().north().east(),
                origin.below().south().west(),
                origin.below().south().east(),
                origin.above().north(),
                origin.above().west(),
                origin.above().south(),
                origin.above().east(),
                origin.above().north().west(),
                origin.above().north().east(),
                origin.above().south().west(),
                origin.above().south().east(),
                origin.above(),
            ]
        );
    }

    #[test]
    fn charge_helpers_match_vanilla_bounds() {
        init_vanilla_registry();

        let empty = vanilla_blocks::RESPAWN_ANCHOR.default_state();
        let partial = empty.set_value(CHARGES, 1);
        let full = empty.set_value(CHARGES, RespawnAnchorBlock::MAX_CHARGES);

        assert!(!RespawnAnchorBlock::has_charge(empty));
        assert!(RespawnAnchorBlock::can_be_charged(empty));
        assert!(RespawnAnchorBlock::has_charge(partial));
        assert!(RespawnAnchorBlock::can_be_charged(partial));
        assert!(RespawnAnchorBlock::has_charge(full));
        assert!(!RespawnAnchorBlock::can_be_charged(full));
    }

    #[test]
    fn charge_consume_after_respawn_matches_vanilla_condition() {
        assert!(RespawnAnchorBlock::should_consume_charge_after_respawn(
            false, true, true,
        ));
        assert!(!RespawnAnchorBlock::should_consume_charge_after_respawn(
            true, true, true,
        ));
        assert!(!RespawnAnchorBlock::should_consume_charge_after_respawn(
            false, false, true,
        ));
        assert!(!RespawnAnchorBlock::should_consume_charge_after_respawn(
            false, true, false,
        ));
    }

    #[test]
    fn consumed_charge_state_decrements_exactly_once() {
        init_vanilla_registry();

        let charged = vanilla_blocks::RESPAWN_ANCHOR
            .default_state()
            .set_value(CHARGES, 2);
        let Some(depleted) = RespawnAnchorBlock::state_after_charge_consumed(charged) else {
            panic!("charged respawn anchor should produce a depleted state");
        };

        assert_eq!(depleted.get_value(CHARGES), 1);
        assert!(
            RespawnAnchorBlock::state_after_charge_consumed(
                vanilla_blocks::RESPAWN_ANCHOR.default_state()
            )
            .is_none()
        );
        assert!(
            RespawnAnchorBlock::state_after_charge_consumed(vanilla_blocks::STONE.default_state())
                .is_none()
        );
    }
}
