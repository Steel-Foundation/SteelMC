use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::{
    BlockRef,
    block_state_ext::BlockStateExt,
    properties::{BlockStateProperties, Direction},
};
use steel_registry::item_stack::ItemStack;
use steel_registry::{sound_events, vanilla_blocks, vanilla_game_events, vanilla_items};
use steel_utils::{
    BlockPos, BlockStateId,
    types::{InteractionHand, UpdateFlags},
};

use crate::entity::dismount_helper;
use crate::{
    behavior::{
        BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
    },
    entity::Entity,
    level_data::RespawnData,
    player::{Player, PlayerRespawnConfig},
    world::{LevelReader, World, game_event::GameEventContext},
};

/// Vanilla respawn anchor
///
/// TODO: Implement vanilla invalid-dimension explosion once Steel has a strict
/// `World::explode` foundation, including block removal, water-sensitive
/// explosion resistance, and bad-respawn-point explosion damage source.
#[block_behavior]
pub struct RespawnAnchorBlock {
    block: BlockRef,
}

impl RespawnAnchorBlock {
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
            && (forced || state.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES) > 0)
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
        let charges = state.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES);
        (charges > 0)
            .then(|| state.set_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES, charges - 1))
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
        state.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES) > 0
    }

    fn can_be_charged(state: BlockStateId) -> bool {
        state.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES) < 4
    }

    fn is_respawn_fuel(item_stack: &ItemStack) -> bool {
        item_stack.is(&vanilla_items::GLOWSTONE)
    }

    const fn consume_respawn_fuel(item_stack: &mut ItemStack, has_infinite_materials: bool) {
        if !has_infinite_materials {
            item_stack.shrink(1);
        }
    }

    fn analog_output_signal(charges: u8) -> i32 {
        i32::from(charges) * 15 / 4
    }

    fn player_offhand_has_respawn_fuel(player: &Player) -> bool {
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::OffHand)
            .is(&vanilla_items::GLOWSTONE)
    }

    fn charge(source: Option<&dyn Entity>, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let charges = state.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES);
        let charged_state =
            state.set_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES, charges + 1);
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
                Self::consume_respawn_fuel(item_stack, has_infinite_materials);
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
            // TODO: Once `World::explode` exist remove the anchor and use the
            // watersensitive bad respawn point explosion behavior
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
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        Self::analog_output_signal(state.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES))
    }
}

#[cfg(test)]
mod tests {
    use super::RespawnAnchorBlock;
    use steel_registry::blocks::{
        block_state_ext::BlockStateExt, properties::BlockStateProperties,
    };
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_items};
    use steel_utils::BlockPos;

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
        init_test_registry();

        let empty = vanilla_blocks::RESPAWN_ANCHOR.default_state();
        let partial = empty.set_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES, 1);
        let full = empty.set_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES, 4);

        assert!(!RespawnAnchorBlock::has_charge(empty));
        assert!(RespawnAnchorBlock::can_be_charged(empty));
        assert!(RespawnAnchorBlock::has_charge(partial));
        assert!(RespawnAnchorBlock::can_be_charged(partial));
        assert!(RespawnAnchorBlock::has_charge(full));
        assert!(!RespawnAnchorBlock::can_be_charged(full));
    }

    #[test]
    fn consuming_respawn_fuel_respects_infinite_materials() {
        init_test_registry();

        let mut survival_fuel = ItemStack::with_count(&vanilla_items::GLOWSTONE, 2);
        RespawnAnchorBlock::consume_respawn_fuel(&mut survival_fuel, false);
        assert_eq!(survival_fuel.count(), 1);

        let mut creative_fuel = ItemStack::with_count(&vanilla_items::GLOWSTONE, 2);
        RespawnAnchorBlock::consume_respawn_fuel(&mut creative_fuel, true);
        assert_eq!(creative_fuel.count(), 2);
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
        init_test_registry();

        let charged = vanilla_blocks::RESPAWN_ANCHOR
            .default_state()
            .set_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES, 2);
        let Some(depleted) = RespawnAnchorBlock::state_after_charge_consumed(charged) else {
            panic!("charged respawn anchor should produce a depleted state");
        };

        assert_eq!(
            depleted.get_value(&BlockStateProperties::RESPAWN_ANCHOR_CHARGES),
            1
        );
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
