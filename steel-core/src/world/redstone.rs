//! Per-world runtime state used by vanilla redstone components.

use std::collections::VecDeque;

use steel_utils::BlockPos;

use super::World;

const RECENT_TOGGLE_TIMER: i64 = 60;
const MAX_RECENT_TOGGLES: usize = 8;

#[derive(Debug, Clone, Copy)]
struct RedstoneTorchToggle {
    pos: BlockPos,
    when: i64,
}

/// Vanilla's per-level `RedstoneTorchBlock.RECENT_TOGGLES` list.
#[derive(Debug, Default)]
pub(super) struct RedstoneTorchToggleTracker {
    toggles: VecDeque<RedstoneTorchToggle>,
}

impl RedstoneTorchToggleTracker {
    fn prune(&mut self, game_time: i64) {
        while self
            .toggles
            .front()
            .is_some_and(|toggle| game_time.wrapping_sub(toggle.when) > RECENT_TOGGLE_TIMER)
        {
            self.toggles.pop_front();
        }
    }

    fn is_toggled_too_frequently(&mut self, pos: BlockPos, game_time: i64, add: bool) -> bool {
        if add {
            self.toggles.push_back(RedstoneTorchToggle {
                pos,
                when: game_time,
            });
        }

        self.toggles
            .iter()
            .filter(|toggle| toggle.pos == pos)
            .take(MAX_RECENT_TOGGLES)
            .count()
            >= MAX_RECENT_TOGGLES
    }
}

impl World {
    /// Removes torch toggles older than vanilla's 60-game-tick window.
    pub(crate) fn prune_recent_redstone_torch_toggles(&self) {
        let game_time = self.game_time();
        self.redstone_torch_toggles.lock().prune(game_time);
    }

    /// Counts recent toggles for `pos`, optionally recording one first.
    ///
    /// This bookkeeping and scheduled tick deadlines both use world game time,
    /// matching Vanilla's loaded-world timing model.
    pub(crate) fn redstone_torch_toggled_too_frequently(&self, pos: BlockPos, add: bool) -> bool {
        let game_time = self.game_time();
        self.redstone_torch_toggles
            .lock()
            .is_toggled_too_frequently(pos, game_time, add)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burnout_requires_eight_toggles_at_the_same_position() {
        let mut tracker = RedstoneTorchToggleTracker::default();
        let pos = BlockPos::new(4, 70, -2);
        for _ in 0..7 {
            assert!(!tracker.is_toggled_too_frequently(pos, 100, true));
        }
        assert!(tracker.is_toggled_too_frequently(pos, 100, true));
        assert!(!tracker.is_toggled_too_frequently(pos.east(), 100, true));
    }

    #[test]
    fn toggle_window_expires_only_after_sixty_game_ticks() {
        let mut tracker = RedstoneTorchToggleTracker::default();
        let pos = BlockPos::new(0, 64, 0);
        for _ in 0..8 {
            tracker.is_toggled_too_frequently(pos, 10, true);
        }

        tracker.prune(70);
        assert!(tracker.is_toggled_too_frequently(pos, 70, false));
        tracker.prune(71);
        assert!(!tracker.is_toggled_too_frequently(pos, 71, false));
    }
}
