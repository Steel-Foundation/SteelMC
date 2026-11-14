//! Vanilla `SimpleExplosionDamageCalculator`: a fixed per-explosion policy.

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::fluid::FluidState;
use steel_utils::{BlockPos, BlockStateId, Identifier};

use crate::entity::Entity;
use crate::world::World;

use super::{
    DefaultExplosionDamageCalculator, Explosion, ExplosionBlockReader, ExplosionDamageCalculator,
    ImmutableExplosionBlockCalculator,
};

const IMMUNE_BLOCK_RESISTANCE: f32 = 3_600_000.0;

/// Explosion policy with fixed block, entity-damage, and knockback decisions.
pub struct SimpleExplosionDamageCalculator {
    explodes_blocks: bool,
    damages_entities: bool,
    knockback_multiplier: Option<f32>,
    /// When set, blocks in this tag fully resist the blast and every other block
    /// offers no resistance at all.
    immune_blocks: Option<Identifier>,
}

impl SimpleExplosionDamageCalculator {
    /// Creates a calculator; `None` values fall back to the default calculator.
    #[must_use]
    pub const fn new(
        explodes_blocks: bool,
        damages_entities: bool,
        knockback_multiplier: Option<f32>,
        immune_blocks: Option<Identifier>,
    ) -> Self {
        Self {
            explodes_blocks,
            damages_entities,
            knockback_multiplier,
            immune_blocks,
        }
    }

    fn resistance(&self, state: BlockStateId, fluid: FluidState) -> Option<f32> {
        let Some(immune_blocks) = &self.immune_blocks else {
            return super::default_block_explosion_resistance(state, fluid);
        };
        state
            .get_block()
            .has_tag(immune_blocks)
            .then_some(IMMUNE_BLOCK_RESISTANCE)
    }
}

impl ExplosionDamageCalculator for SimpleExplosionDamageCalculator {
    fn block_explosion_resistance(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        self.resistance(state, fluid)
    }

    fn should_block_explode(
        &self,
        _explosion: &dyn Explosion,
        _world: &World,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.explodes_blocks
    }

    fn should_damage_entity(&self, _explosion: &dyn Explosion, _entity: &dyn Entity) -> bool {
        self.damages_entities
    }

    fn knockback_multiplier(&self, entity: &dyn Entity) -> f32 {
        if entity.is_flying_player() {
            return 0.0;
        }
        self.knockback_multiplier
            .unwrap_or_else(|| DefaultExplosionDamageCalculator.knockback_multiplier(entity))
    }
}

impl ImmutableExplosionBlockCalculator for SimpleExplosionDamageCalculator {
    fn bounded_block_read_radius(&self) -> Option<u32> {
        Some(0)
    }

    fn can_cache_explosion_resistance(&self) -> bool {
        true
    }

    fn always_allows_block_explosion(&self) -> bool {
        self.explodes_blocks
    }

    fn explosion_resistance(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
    ) -> Option<f32> {
        self.resistance(state, fluid)
    }

    fn should_explode(
        &self,
        _reader: &dyn ExplosionBlockReader,
        _pos: BlockPos,
        _state: BlockStateId,
        _power: f32,
    ) -> bool {
        self.explodes_blocks
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::fluid::FluidState;
    use steel_registry::vanilla_block_tags::BlockTag;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::BlockPos;

    use super::*;
    use crate::test_support::test_world;

    #[test]
    fn immune_block_tag_leaves_every_other_block_without_resistance() {
        init_vanilla_registry();

        let calculator = SimpleExplosionDamageCalculator::new(
            true,
            false,
            None,
            Some(BlockTag::BLOCKS_WIND_CHARGE_EXPLOSIONS),
        );
        let reader: &dyn ExplosionBlockReader = test_world().as_ref();
        let pos = BlockPos::new(0, 64, 0);
        let obsidian = vanilla_blocks::OBSIDIAN.default_state();

        assert!(
            DefaultExplosionDamageCalculator
                .explosion_resistance(reader, pos, obsidian, FluidState::EMPTY)
                .is_some()
        );
        assert_eq!(
            calculator.explosion_resistance(reader, pos, obsidian, FluidState::EMPTY),
            None
        );
        assert_eq!(
            calculator.explosion_resistance(
                reader,
                pos,
                vanilla_blocks::BARRIER.default_state(),
                FluidState::EMPTY
            ),
            Some(IMMUNE_BLOCK_RESISTANCE)
        );
    }
}
