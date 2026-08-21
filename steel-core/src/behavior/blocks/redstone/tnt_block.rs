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

const PRIMING_SOUND_VOLUME: f32 = 1.0;
const PRIMING_SOUND_PITCH: f32 = 1.0;
const FLINT_AND_STEEL_DAMAGE_PER_USE: i32 = 1;
const FIRE_CHARGE_ITEMS_PER_USE: i32 = 1;

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
        let (x, y, z) = pos.get_bottom_center();
        Arc::new(PrimedTntEntity::primed(
            &vanilla_entities::TNT,
            next_entity_id(),
            DVec3::new(x, y, z),
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
            PRIMING_SOUND_VOLUME,
            PRIMING_SOUND_PITCH,
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
                item.hurt_and_break(FLINT_AND_STEEL_DAMAGE_PER_USE, has_infinite_materials);
            } else if !has_infinite_materials {
                item.shrink(FIRE_CHARGE_ITEMS_PER_USE);
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
#[path = "tnt_block/tests.rs"]
mod tests;
