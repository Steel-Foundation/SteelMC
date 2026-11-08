//! Vanilla `BlockAttachedEntity`: entities fixed to a block that pop off when
//! moved, pushed or hurt, and when their block can no longer hold them.
//!
//! State lives in the [`BlockAttachment`] component; entity types implement
//! [`Parts`] and start their hooks from [`entity_props`].

use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::vanilla_damage_types;
use steel_registry::vanilla_game_events;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::entity::component::{Component, Has};
use crate::entity::damage::DamageSource;
use crate::entity::prelude::*;
use crate::entity::{Entity, EntityMoveError, EntityProps, RemovalReason, SharedEntity};
use crate::world::{Explosion, World};

/// Ticks between two survival checks.
const CHECK_INTERVAL: u32 = 100;

struct AttachmentState {
    ticks_since_last_check: u32,
    pos: BlockPos,
}

/// The block a vanilla `BlockAttachedEntity` hangs on.
pub struct BlockAttachment {
    state: CheckedMutex<AttachmentState>,
}

impl BlockAttachment {
    /// Attached to `pos`.
    #[must_use]
    pub const fn new(pos: BlockPos) -> Self {
        Self {
            state: CheckedMutex::new(AttachmentState {
                ticks_since_last_check: 0,
                pos,
            }),
        }
    }

    /// The block this entity is attached to (vanilla `getPos`).
    #[must_use]
    pub fn pos(&self) -> BlockPos {
        self.state.lock().pos
    }

    fn set_pos(&self, pos: BlockPos) {
        self.state.lock().pos = pos;
    }

    /// Advances the check timer, returning whether this tick checks survival.
    fn advance_check_timer(&self) -> bool {
        let mut state = self.state.lock();
        let due = state.ticks_since_last_check >= CHECK_INTERVAL;
        state.ticks_since_last_check = if due {
            0
        } else {
            state.ticks_since_last_check + 1
        };
        due
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `BlockAttachment`.
unsafe impl DowncastType for BlockAttachment {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:entity_component/block_attachment");
}

impl Component for BlockAttachment {
    fn save(&self, _owner: &dyn Entity, nbt: &mut NbtCompound) {
        let pos = self.pos();
        nbt.insert(
            "block_pos",
            NbtTag::IntArray(vec![pos.x(), pos.y(), pos.z()]),
        );
    }

    fn load(&self, owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let stored = nbt
            .int_array("block_pos")
            .filter(|pos| pos.len() == 3)
            .map(|pos| BlockPos::new(pos[0], pos[1], pos[2]));
        match stored {
            Some(pos) if pos.closer_than(owner.block_position(), 16.0) => {
                self.set_pos(pos);
            }
            _ => log::error!("Block-attached entity at invalid position: {stored:?}"),
        }
    }
}

/// What a block-attached entity type defines on top of the shared layer.
pub trait Parts: Entity + Has<BlockAttachment> {
    /// Whether the entity can stay attached; checked every
    /// [`CHECK_INTERVAL`] ticks.
    fn survives(&self) -> bool;

    /// Drops what the entity leaves behind when it pops off.
    fn drop_item(&self, world: &World, caused_by: Option<&dyn Entity>);

    /// Places the entity and its bounding box around its block.
    fn recalculate_bounding_box(&self) -> Result<(), EntityMoveError>;

    /// The block this entity is attached to.
    fn block_pos(&self) -> BlockPos {
        self.component().pos()
    }
}

/// The `BlockAttachedEntity` overrides of the entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        tick: |entity, _| tick(entity),
        is_pickable: |_| true,
        skip_attack_interaction: skip_attack_interaction::<E>,
        hurt: hurt::<E>,
        ignore_explosion: |_, explosion| ignore_explosion(explosion),
        move_entity: |entity, _, _, delta| {
            pop_off_if_moved(entity, delta);
            None
        },
        push_impulse: pop_off_if_moved::<E>,
        refresh_dimensions: |_| {},
        try_set_position: set_position::<E>,
        // Every vanilla subclass spawns on clients at its block, not its position.
        spawn_position: |entity| entity.block_pos().0.as_dvec3(),
        ..EntityProps::new()
    }
}

fn tick<E: Parts>(entity: &E) {
    let Some(world) = entity.level() else {
        return;
    };
    entity.check_below_world();
    if entity.component().advance_check_timer() && !entity.is_removed() && !entity.survives() {
        entity.set_removed(RemovalReason::Discarded);
        entity.drop_item(&world, None);
    }
}

fn skip_attack_interaction<E: Parts>(entity: &E, source: &SharedEntity) -> bool {
    let Some(player) = source.as_player() else {
        return false;
    };
    let Some(world) = entity.level() else {
        return false;
    };
    let damage_source =
        DamageSource::direct(&vanilla_damage_types::PLAYER_ATTACK, Arc::clone(source));
    !world.may_interact(player, entity.block_pos()) || entity.hurt(&world, &damage_source, 0.0)
}

fn hurt<E: Parts>(entity: &E, world: &World, source: &DamageSource, _amount: f32) -> bool {
    if entity.is_invulnerable_to_base(source) {
        return false;
    }
    let causing_entity = source.causing_entity().map(AsRef::as_ref);
    if !world.get_game_rule(&MOB_GRIEFING) && causing_entity.is_some_and(Entity::is_mob) {
        return false;
    }
    if !entity.is_removed() {
        kill_attributed(entity, causing_entity);
        entity.mark_hurt();
        entity.drop_item(world, causing_entity);
    }
    true
}

/// Vanilla `BlockAttachedEntity.kill(level, attributedTo)`: the death game
/// event names the attacker instead of the entity itself.
fn kill_attributed(entity: &dyn Entity, attributed_to: Option<&dyn Entity>) {
    entity.set_removed(RemovalReason::Killed);
    entity.game_event_with_source_entity(
        &vanilla_game_events::ENTITY_DIE,
        Some(attributed_to.unwrap_or(entity)),
    );
}

fn ignore_explosion(explosion: &dyn Explosion) -> bool {
    explosion
        .direct_source_entity()
        .is_some_and(EntityFluids::is_in_water)
        || !explosion.should_affect_blocklike_entities()
}

/// Vanilla `move` and `push`: any movement knocks the entity off its block.
fn pop_off_if_moved<E: Parts>(entity: &E, delta: DVec3) {
    let Some(world) = entity.level() else {
        return;
    };
    if entity.is_removed() || delta.length_squared() <= 0.0 {
        return;
    }
    entity.kill(&world);
    entity.drop_item(&world, None);
}

/// Vanilla `setPos`: moves the attachment to the block containing `pos` and
/// recomputes the position from it.
fn set_position<E: Parts>(entity: &E, pos: DVec3) -> Result<(), EntityMoveError> {
    entity
        .component()
        .set_pos(BlockPos::containing(pos.x, pos.y, pos.z));
    entity.recalculate_bounding_box()?;
    entity.base().mark_velocity_sync();
    Ok(())
}

#[cfg(test)]
mod tests {
    use steel_utils::BlockPos;

    use super::BlockAttachment;

    #[test]
    fn survival_is_checked_on_the_tick_after_the_interval() {
        let attachment = BlockAttachment::new(BlockPos::new(0, 0, 0));
        for _ in 0..100 {
            assert!(!attachment.advance_check_timer());
        }
        assert!(attachment.advance_check_timer());
        assert!(!attachment.advance_check_timer());
    }
}
