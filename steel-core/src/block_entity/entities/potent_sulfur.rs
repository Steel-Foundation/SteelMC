//! `PotentSulfurBlockEntity` for geyser eruption

use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, PotentSulfurState};
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_game_events;
use steel_registry::{
    REGISTRY, TaggedRegistryExt as _, vanilla_blocks, vanilla_entity_type_tags::EntityTypeTag,
};
use steel_utils::random::xoroshiro::Xoroshiro;
use steel_utils::random::{PositionalRandom, Random, RandomSource, RandomSplitter};
use steel_utils::types::UpdateFlags;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, WorldAabb, locks::SyncMutex,
};

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::block_entity::{BlockEntity, BlockEntityBase, BlockEntityLifecycleExt as _};
use crate::fluid::FluidStateExt as _;
use crate::world::World;

const GEYSER_SALT: i64 = -904_011_478;
const COUNTDOWN_FREQUENCY_TICKS: i64 = 20;
const LAUNCH_FORCE: f64 = 0.2;
const BASE_VELOCITY_THRESHOLD: f64 = 0.3;
const VELOCITY_THRESHOLD_SCALE: f64 = 0.1;
const MAX_WATER_BLOCKS_ABOVE: i32 = 4;
const FORCE_HEIGHT_MULTIPLIER: i32 = 6;

/// Block entity for `potent_sulfur` blocks
pub struct PotentSulfurBlockEntity {
    base: BlockEntityBase,
    sulfur: SyncMutex<PotentSulfurData>,
}

struct PotentSulfurData {
    /// Countdown for 20 tick steps before the state toggles. -1 is uninitialized
    waiting_countdown: i32,
    /// Game tick at which the current eruption started
    eruption_tick: i64,
}

// SAFETY: This key is owned by Steel and uniquely identifies `PotentSulfurBlockEntity`.
unsafe impl DowncastType for PotentSulfurBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/potent_sulfur");
}

impl PotentSulfurBlockEntity {
    /// Creates a new block entity
    #[must_use]
    pub fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let eruption_tick = world.upgrade().map_or(-1, |w| w.game_time());
        Self {
            base: BlockEntityBase::new(
                &vanilla_block_entity_types::POTENT_SULFUR,
                world,
                pos,
                state,
            ),
            sulfur: SyncMutex::new(PotentSulfurData {
                waiting_countdown: -1,
                eruption_tick,
            }),
        }
    }

    /// Resets the countdown so it reinitializes on the next tick
    pub fn reset_countdown(&self) {
        self.sulfur.lock().waiting_countdown = -1;
    }

    /// Records the tick at which an eruption started.
    pub fn set_eruption_tick(&self, eruption_tick: i64) {
        self.sulfur.lock().eruption_tick = eruption_tick;
    }

    fn geyser_positional_rng(seed: i64, pos: BlockPos) -> Xoroshiro {
        let mut base = Xoroshiro::from_seed((seed ^ GEYSER_SALT) as u64);
        let RandomSplitter::Xoroshiro(splitter) = base.next_positional() else {
            unreachable!("Xoroshiro always produces Xoroshiro splitter")
        };
        match splitter.at(pos.x(), pos.y(), pos.z()) {
            RandomSource::Xoroshiro(r) => r,
            RandomSource::Legacy(_) => {
                unreachable!("XoroshiroSplitter::at always returns Xoroshiro")
            }
        }
    }

    fn is_geyser_passable(world: &World, pos: BlockPos, context: BlockCollisionContext) -> bool {
        let state = world.get_block_state(pos);
        if state.is_air() || state.get_block() == &vanilla_blocks::WATER {
            return true;
        }

        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        behavior
            .get_collision_shape(state, world, pos, context)
            .is_empty()
    }

    fn find_source_block(world: &World, origin: BlockPos) -> Option<BlockPos> {
        let max_y = origin.y() + MAX_WATER_BLOCKS_ABOVE + 1;
        let geyser_position_context =
            BlockCollisionContext::position_context(f64::from(origin.y()));
        let mut pos = BlockPos::new(origin.x(), origin.y() + 1, origin.z());

        while pos.y() <= max_y {
            let state = world.get_block_state(pos);
            let fluid = state.get_fluid_state();
            let is_water_source = fluid.is_source() && fluid.is_water();

            if is_water_source
                && (state.get_block() == &vanilla_blocks::WATER
                    || Self::is_geyser_passable(world, pos, geyser_position_context))
            {
                pos = BlockPos::new(pos.x(), pos.y() + 1, pos.z());
                continue;
            }

            if state.is_air() || Self::is_geyser_passable(world, pos, geyser_position_context) {
                return Some(pos);
            }

            break; // Solid obstruction
        }

        None
    }

    fn unobstructed_block_count(world: &World, start: BlockPos, water_blocks: i32) -> i32 {
        let max_height = FORCE_HEIGHT_MULTIPLIER * water_blocks;
        let geyser_position_context =
            BlockCollisionContext::position_context(f64::from(start.y() - 1));
        for i in 0..max_height {
            let check = BlockPos::new(start.x(), start.y() + i, start.z());
            if !Self::is_geyser_passable(world, check, geyser_position_context) {
                return i;
            }
        }
        max_height
    }

    fn tick_countdown(
        &self,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<(BlockStateId, bool)> {
        let source = Self::find_source_block(world, pos)?;
        let water_blocks = source.y() - pos.y() - 1;
        let mut rng = Self::geyser_positional_rng(world.seed(), pos);
        let game_time = world.game_time();
        let mut sulfur = self.sulfur.lock();

        if sulfur.waiting_countdown <= 0 {
            let current_state = state.get_value(&BlockStateProperties::POTENT_SULFUR_STATE);

            sulfur.waiting_countdown = if current_state == PotentSulfurState::Dormant {
                10 * (water_blocks - 1) + rng.next_i32_between(15, 30)
            } else {
                rng.next_i32();
                (water_blocks - 1) + rng.next_i32_between(1, 2)
            };
        }

        if sulfur.waiting_countdown > 0 {
            sulfur.waiting_countdown -= 1;
        }

        if sulfur.waiting_countdown == 0 {
            let current_state = state.get_value(&BlockStateProperties::POTENT_SULFUR_STATE);
            let next_state = if current_state == PotentSulfurState::Dormant {
                PotentSulfurState::Erupting
            } else {
                PotentSulfurState::Dormant
            };
            let deactivates = next_state == PotentSulfurState::Dormant;
            let activates = next_state == PotentSulfurState::Erupting;
            let new_state = state.set_value(&BlockStateProperties::POTENT_SULFUR_STATE, next_state);
            if activates {
                sulfur.eruption_tick = game_time;
            }
            return Some((new_state, deactivates));
        }

        None
    }

    fn tick_launch(world: &Arc<World>, pos: BlockPos) {
        let Some(source) = Self::find_source_block(world, pos) else {
            return;
        };

        let water_blocks = source.y() - pos.y() - 1;
        let above = BlockPos::new(pos.x(), pos.y() + 1, pos.z());
        let force_height = Self::unobstructed_block_count(world, above, water_blocks);

        let aabb = WorldAabb::new(
            f64::from(pos.x()),
            f64::from(pos.y() + 1),
            f64::from(pos.z()),
            f64::from(pos.x() + 1),
            f64::from(pos.y() + 1) + f64::from(force_height),
            f64::from(pos.z() + 1),
        );

        let velocity_threshold =
            BASE_VELOCITY_THRESHOLD + f64::from(water_blocks) * VELOCITY_THRESHOLD_SCALE;

        for entity in world.get_entities_in_aabb(&aabb) {
            if !entity.is_alive() || entity.is_spectator() {
                continue;
            }
            let vel = entity.velocity();
            entity.check_fall_distance_accumulation();

            if !entity.can_simulate_movement() {
                continue;
            }
            if entity.is_flying_player() {
                continue;
            }
            if entity.is_passenger() {
                continue;
            }
            if REGISTRY.entity_types.is_in_tag(
                entity.entity_type(),
                &EntityTypeTag::NOT_AFFECTED_BY_GEYSERS,
            ) {
                continue;
            }
            if vel.y >= velocity_threshold {
                continue;
            }

            entity.set_velocity(glam::DVec3::new(vel.x, vel.y + LAUNCH_FORCE, vel.z));
            entity.mark_velocity_sync();
        }
    }
}

impl BlockEntity for PotentSulfurBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        if let Some(countdown) = nbt.int("countdown") {
            self.sulfur.lock().waiting_countdown = countdown;
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("countdown", self.sulfur.lock().waiting_countdown);
    }

    fn tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let state = world.get_block_state(pos);
        if state.get_block() != &vanilla_blocks::POTENT_SULFUR {
            self.set_removed();
            return;
        }

        let current = state.get_value(&BlockStateProperties::POTENT_SULFUR_STATE);

        if current == PotentSulfurState::Dry {
            return;
        }

        let game_time = world.game_time();

        // TODO: Add nausea ticker (WET / DORMANT states, every 10 ticks) after the mob-effect refactor adds timed instances and sync.

        let action = if matches!(
            &current,
            PotentSulfurState::Dormant | PotentSulfurState::Erupting
        ) && game_time % COUNTDOWN_FREQUENCY_TICKS == 0
        {
            self.tick_countdown(world, pos, state)
        } else {
            None
        };

        if matches!(
            &current,
            PotentSulfurState::Erupting | PotentSulfurState::Continuous
        ) {
            Self::tick_launch(world, pos);
        }

        if let Some((new_state, deactivates)) = action {
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
            if deactivates {
                world.game_event(
                    &vanilla_game_events::BLOCK_DEACTIVATE,
                    pos,
                    &crate::world::game_event_context::GameEventContext::new(None, Some(state)),
                );
            }
        }
    }
}
