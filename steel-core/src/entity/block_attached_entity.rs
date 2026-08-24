//! Shared Vanilla `BlockAttachedEntity` state and hooks.
//!
//! These entities store an additional counter, incrementing each tick,
//! checking whether it can survive when it reaches the maximum value.
//! They are also bound to a position, and cannot move.

use crate::entity::damage::DamageSource;
use crate::entity::{BorrowedNbtCompoundView, EntityMoveError};
use crate::entity::{Entity, RemovalReason};
use crate::physics::{MoveResult, MoverType};
use crate::world::World;
use glam::DVec3;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::vanilla_damage_types;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::BlockPos;
use steel_utils::locks::SyncMutex;

pub const VALID_READ_DISTANCE_SQR: f64 = 16.0 * 16.0;

/// The time interval between two consecutive survival checks for block-attached entities.
///
/// This is equal to 5 seconds in ticks, or `100`.
pub const CHECK_SURVIVAL_FREQUENCY_TICKS: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockAttachedEntityState {
    check_interval: u32,
    pos: BlockPos,
}

impl BlockAttachedEntityState {
    pub const fn new(pos: BlockPos) -> Self {
        Self {
            check_interval: 0,
            pos,
        }
    }
}

/// Runtime fields shared by Vanilla's block-attached entities (those inheriting `BlockAttachedEntity` in Vanilla).
#[derive(Debug)]
pub struct BlockAttachedEntityBase {
    state: SyncMutex<BlockAttachedEntityState>,
}

impl BlockAttachedEntityBase {
    /// Creates a new [`BlockAttachedEntityBase`] with the given position.
    pub const fn new(pos: BlockPos) -> Self {
        BlockAttachedEntityBase {
            state: SyncMutex::new(BlockAttachedEntityState::new(pos)),
        }
    }

    /// Gets the position of this base's entity.
    pub fn pos(&self) -> BlockPos {
        self.state.lock().pos
    }

    /// Sets the position of this base's entity to something.
    pub fn set_pos(&self, pos: BlockPos) {
        self.state.lock().pos = pos;
    }

    /// Increments the internal counter of this base, returning whether to check for survival.
    /// - If it is the maximum value of [`CHECK_SURVIVAL_FREQUENCY_TICKS`], this returns `true`, and the counter is reset.
    /// - If not, this returns `false`, and increments the counter.
    pub fn should_check_survival(&self) -> bool {
        let mut state = self.state.lock();

        if state.check_interval == CHECK_SURVIVAL_FREQUENCY_TICKS {
            state.check_interval = 0;
            true
        } else {
            state.check_interval += 1;
            false
        }
    }
}

pub trait BlockAttachedEntity: Entity {
    /// Returns the shared block-attached entity runtime state.
    fn block_attached_entity_base(&self) -> &BlockAttachedEntityBase;

    fn survives(&self) -> bool;

    fn drop_item(&self, caused_by: Option<&dyn Entity>);

    fn recalculate_bounding_box(&self) -> Result<(), EntityMoveError>;

    fn tick_block_attached_entity(&self) {
        self.check_below_world();
        if self.block_attached_entity_base().should_check_survival() {
            // Check for survival.
            if !self.is_removed() && !self.survives() {
                self.set_removed(RemovalReason::Discarded);
                self.drop_item(None);
            }
        }
    }

    fn is_pickable_block_attached_entity(&self) -> bool {
        true
    }

    fn skip_attack_interaction_block_attached_entity(&self, source: &dyn Entity) -> bool {
        if let Some(player) = source.as_player()
            && let Some(world) = source.level()
        {
            let damage_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
                .with_causing_entity(player.id())
                .with_direct_entity(player.id());

            !world.may_interact(player, self.block_attached_entity_base().pos())
                || self.hurt(&world, &damage_source, 0.0)
        } else {
            false
        }
    }

    fn hurt_block_attached_entity(
        &self,
        world: &World,
        source: &DamageSource,
        _amount: f32,
    ) -> bool {
        if self.default_is_invulnerable_to(source) {
            return false;
        }

        let causing_entity = source
            .causing_entity_id
            .and_then(|id| world.get_entity_by_id(id));

        if !world.get_game_rule(&MOB_GRIEFING)
            && let Some(causing_entity) = &causing_entity
            && causing_entity.is_mob()
        {
            return false;
        }

        if !self.is_removed() {
            self.kill(world);
            self.mark_hurt();
            self.drop_item(causing_entity.as_deref());
        }

        true
    }

    fn drop_if_vec3_is_nonzero(&self, impulse: DVec3) {
        if let Some(world) = self.level()
            && !self.is_removed()
            && impulse.length_squared() > 0.0
        {
            self.kill(&world);
            self.drop_item(None);
        }
    }

    fn move_entity_block_attached_entity(
        &self,
        _mover_type: MoverType,
        delta: DVec3,
    ) -> Option<MoveResult> {
        self.drop_if_vec3_is_nonzero(delta);
        None
    }

    fn push_impulse_block_attached_entity(&self, impulse: DVec3) {
        self.drop_if_vec3_is_nonzero(impulse);
    }

    fn save_block_attached_entity(&self, nbt: &mut NbtCompound) {
        let block_pos = self.block_attached_entity_base().pos();
        nbt.insert(
            "block_pos",
            NbtTag::IntArray(vec![block_pos.x(), block_pos.y(), block_pos.z()]),
        );
    }

    fn load_block_attached_entity(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(block_pos_vec) = nbt.int_array("block_pos")
            && block_pos_vec.len() == 3
        {
            let block_pos = BlockPos::new(block_pos_vec[0], block_pos_vec[1], block_pos_vec[2]);
            self.block_attached_entity_base().set_pos(block_pos);
            if block_pos.distance_sqr(block_pos.0.as_dvec3()) >= VALID_READ_DISTANCE_SQR {
                log::warn!("Block-attached entity at invalid position: {block_pos:?}");
            }
        }
    }

    fn refresh_dimensions_block_attached_entity(&self) {}

    fn try_set_position_block_attached_entity(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.block_attached_entity_base()
            .set_pos(BlockPos(pos.as_ivec3()));
        self.recalculate_bounding_box()?;
        self.base().mark_velocity_sync();
        Ok(())
    }

    // TODO: Add a default implementation for `thunderHit` and `ignoreExplosion`
}
