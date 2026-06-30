//! Fire block behavior implementation.
//!
//! Vanilla splits fire into `BaseFireBlock` (portal logic, placement checks) and `FireBlock`
//! (spreading, aging). This combines the portal-relevant parts from `BaseFireBlock`.

use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_damage_types;
use steel_registry::vanilla_dimension_types;
use steel_utils::axis::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, InsideBlockEffectCollector, InsideBlockEffectType};
use crate::portal::portal_shape::{PortalShape, nether_portal_config};
use crate::world::{LevelReader, World};

/// Behavior for fire blocks.
#[block_behavior]
pub struct FireBlock {
    block: BlockRef,
}

impl FireBlock {
    /// Creates a new fire block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns true if the world supports nether portal creation.
    ///
    /// Vanilla expresses this in terms of dimensions; Steel checks the loaded
    /// world's vanilla dimension type.
    pub(crate) fn in_portal_world(world: &World) -> bool {
        world.dimension_type == &vanilla_dimension_types::OVERWORLD
            || world.dimension_type == &vanilla_dimension_types::THE_NETHER
    }

    /// Checks if fire can be placed at `pos`, matching vanilla's `BaseFireBlock.canBePlacedAt`.
    /// Position must be air AND (fire can survive there OR it's a valid portal location).
    pub(crate) fn can_be_placed_at(
        world: &Arc<World>,
        pos: BlockPos,
        forward_dir: Direction,
    ) -> bool {
        if !world.get_block_state(pos).is_air() {
            return false;
        }
        Self::selected_fire_can_survive_at(world.as_ref(), pos)
            || Self::is_portal(world, pos, forward_dir)
    }

    /// Steel equivalent of vanilla's `BaseFireBlock.getState` for selecting
    /// between soul fire and regular fire.
    pub(crate) fn get_state(world: &dyn LevelReader, pos: BlockPos) -> BlockStateId {
        if SoulFireBlock::can_survive_at(world, pos) {
            vanilla_blocks::SOUL_FIRE.default_state()
        } else {
            vanilla_blocks::FIRE.default_state()
        }
    }

    fn selected_fire_can_survive_at(world: &dyn LevelReader, pos: BlockPos) -> bool {
        SoulFireBlock::can_survive_at(world, pos) || Self::can_survive_at(world, pos)
    }

    /// Matches vanilla's `FireBlock.canSurvive`: block below has a sturdy top face,
    /// or an adjacent block is flammable.
    fn can_survive_at(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        world
            .get_block_state(below_pos)
            .is_face_sturdy_at(below_pos, Direction::Up)
        // TODO: || is_valid_fire_location (check adjacent flammable blocks once flammability exists)
    }

    /// Matches vanilla's `BaseFireBlock.isPortal`: checks if placing fire here could form a portal.
    /// Requires a portal-capable world, adjacent obsidian, and a valid empty portal shape.
    fn is_portal(world: &Arc<World>, pos: BlockPos, forward_dir: Direction) -> bool {
        if !Self::in_portal_world(world) {
            return false;
        }

        let has_obsidian = Direction::ALL.iter().any(|&dir| {
            world.get_block_state(pos.relative(dir)).get_block() == &vanilla_blocks::OBSIDIAN
        });
        if !has_obsidian {
            return false;
        }

        let preferred_axis = if forward_dir.get_axis().is_horizontal() {
            forward_dir.rotate_y_counter_clockwise().get_axis()
        } else if rand::random::<bool>() {
            Axis::X
        } else {
            Axis::Z
        };

        let config = nether_portal_config();
        PortalShape::find_empty_portal_shape_with_axis(world, pos, preferred_axis, &config)
            .is_some()
    }

    fn queue_entity_contact_effects(
        effect_collector: &mut InsideBlockEffectCollector,
        fire_damage: f32,
    ) {
        effect_collector.apply(InsideBlockEffectType::ClearFreeze);
        effect_collector.apply(InsideBlockEffectType::FireIgnite);
        effect_collector.run_after(
            InsideBlockEffectType::FireIgnite,
            Box::new(move |entity| {
                if !entity.fire_immune() {
                    entity.hurt(
                        &DamageSource::environment(&vanilla_damage_types::IN_FIRE),
                        fire_damage,
                    );
                }
            }),
        );
    }
}

impl BlockBehavior for FireBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if SoulFireBlock::can_survive_at(context.world.as_ref(), context.relative_pos) {
            Some(vanilla_blocks::SOUL_FIRE.default_state())
        } else {
            Some(self.block.default_state())
        }
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::can_survive_at(world, pos)
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        Self::queue_entity_contact_effects(effect_collector, 1.0);
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        // Only attempt portal creation when fire is newly placed, not when replacing itself
        if old_state.get_block() == state.get_block() {
            return;
        }

        if Self::in_portal_world(world)
            && let Some(shape) =
                PortalShape::find_empty_portal_shape(world, pos, &nether_portal_config())
        {
            shape.place_portal_blocks(world);
            return;
        }

        if !self.can_survive(state, world, pos) {
            world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }
}

/// Behavior for soul fire survival.
///
/// Vanilla keeps this as `SoulFireBlock`, separate from normal `FireBlock`.
/// Spread/burn behavior is still TODO with the rest of fire ticking.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct SoulFireBlock {
    block: BlockRef,
}

impl SoulFireBlock {
    /// Creates a new soul fire block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn can_survive_at(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let block_below = world.get_block_state(pos.below()).get_block();
        block_below.has_tag(&BlockTag::SOUL_FIRE_BASE_BLOCKS)
    }
}

impl BlockBehavior for SoulFireBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state)
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::can_survive_at(world, pos)
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        FireBlock::queue_entity_contact_effects(effect_collector, 2.0);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        blocks::block_state_ext::BlockStateExt, test_support::init_test_registry, vanilla_blocks,
    };
    use steel_utils::{BlockPos, BlockStateId};

    use crate::test_support::TestLevel;

    use super::FireBlock;

    const POS: BlockPos = BlockPos::new(0, 64, 0);

    fn level_with_support(support_state: BlockStateId) -> TestLevel {
        TestLevel::default()
            .with_min_y(0)
            .with_block(POS.below(), support_state)
    }

    #[test]
    fn get_state_selects_soul_fire_on_soul_fire_base_block() {
        init_test_registry();

        let level = level_with_support(vanilla_blocks::SOUL_SAND.default_state());

        assert_eq!(
            FireBlock::get_state(&level, POS).get_block(),
            &vanilla_blocks::SOUL_FIRE
        );
        assert!(FireBlock::selected_fire_can_survive_at(&level, POS));
    }

    #[test]
    fn get_state_selects_regular_fire_otherwise() {
        init_test_registry();

        let level = level_with_support(vanilla_blocks::STONE.default_state());

        assert_eq!(
            FireBlock::get_state(&level, POS).get_block(),
            &vanilla_blocks::FIRE
        );
    }
}
