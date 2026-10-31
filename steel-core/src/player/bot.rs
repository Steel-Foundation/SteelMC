//! Host-controlled players retain normal living physics, tools, reach, and mining speed.
//! Input expires after one actor tick; extensions must keep supplying active controls.
use super::game_mode::block_breaking::{BlockBreakAction, get_destroy_progress};
use crate::{
    entity::{Entity, LivingEntity, LivingTravelInput},
    player::{Player, PlayerInput},
};
use std::mem;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, properties::Direction},
    vanilla_blocks,
};
use steel_utils::ChunkPos;
use steel_utils::{BlockPos, locks::SyncMutex};
/// Input for one server-controlled player tick.
#[derive(Clone, Copy, Debug)]
pub struct BotInput {
    /// Rotation in degrees.
    pub yaw: f32,
    /// Vertical look angle in degrees.
    pub pitch: f32,
    /// Movement and jump intent, using the same representation as a real client.
    pub movement: PlayerInput,
    /// Block to mine while this input remains supplied.
    pub dig: Option<BlockPos>,
}
impl Default for BotInput {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            movement: PlayerInput::EMPTY,
            dig: None,
        }
    }
}
#[derive(Default)]
pub(crate) struct BotController {
    state: SyncMutex<BotState>,
}
#[derive(Default)]
struct BotState {
    input: BotInput,
    target: Option<BlockPos>,
    progress: f32,
}
impl Player {
    /// Whether this actor is controlled by the host instead of a network client.
    #[must_use]
    pub const fn is_server_controlled(&self) -> bool {
        self.bot.is_some()
    }
    /// Supplies input for the next tick without moving the actor immediately.
    pub fn control_bot(&self, input: BotInput) -> Result<(), String> {
        let Some(bot) = &self.bot else {
            return Err("player is not server controlled".into());
        };
        if !input.yaw.is_finite() || !input.pitch.is_finite() {
            return Err("bot rotation must be finite".into());
        }
        bot.state.lock().input = input;
        Ok(())
    }
    pub(super) fn tick_bot_controls(&self) {
        let Some(bot) = &self.bot else {
            return;
        };
        let (input, previous, mut progress) = {
            let mut state = bot.state.lock();
            let input = mem::take(&mut state.input);
            (input, state.target, state.progress)
        };
        self.set_rotation((input.yaw, input.pitch.clamp(-90.0, 90.0)));
        self.movement.lock().set_last_client_input(input.movement);
        self.set_travel_input(LivingTravelInput::new(
            input.movement.left_intent(),
            0.0,
            input.movement.forward_intent(),
        ));
        self.set_jumping(input.movement.jump());
        self.set_sprinting(input.movement.sprint());
        let world = self.get_world();
        let target = input.dig.filter(|pos| {
            self.is_within_block_interaction_range(*pos)
                && world
                    .chunk_map
                    .with_full_chunk(ChunkPos::from_block_pos(*pos), |_| ())
                    .is_some()
                && world.get_block_state(*pos).get_block() != &vanilla_blocks::AIR
        });
        if previous != target {
            if let Some(pos) = previous {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    pos,
                    BlockBreakAction::Abort,
                    Direction::Up,
                );
            }
            progress = 0.0;
            if let Some(pos) = target {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    pos,
                    BlockBreakAction::Start,
                    Direction::Up,
                );
            }
        }
        if let Some(pos) = target {
            progress += get_destroy_progress(self, world.get_block_state(pos));
            if progress >= 1.0 {
                self.block_breaking.lock().handle_block_break_action(
                    self,
                    &world,
                    pos,
                    BlockBreakAction::Stop,
                    Direction::Up,
                );
            }
        }
        let mut state = bot.state.lock();
        state.target = target;
        state.progress = progress;
    }
}
