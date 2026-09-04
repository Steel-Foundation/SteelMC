use glam::DVec3;
use steel_protocol::packets::game::{AnimateAction, CAnimate};
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt as _, properties::BlockStateProperties},
    dimension_type::BedRuleValue,
    vanilla_custom_stats,
};
use steel_utils::{BlockPos, Direction};
use text_components::{TextComponent, translation::TranslatedMessage};

use super::sleep_state::SLEEP_DURATION;
use super::{Player, PlayerRespawnConfig};
use crate::{
    entity::{Entity, LivingEntity as _},
    level_data::RespawnData,
    world::World,
};

const BED_INTERACTION_XZ_RANGE: f64 = 3.0;
const BED_INTERACTION_Y_RANGE: f64 = 2.0;

#[derive(Debug)]
pub(crate) enum BedSleepingProblem {
    OtherProblem,
    Message(Box<TextComponent>),
}

impl BedSleepingProblem {
    #[must_use]
    pub(crate) fn message(&self) -> Option<&TextComponent> {
        match self {
            Self::Message(message) => Some(message.as_ref()),
            Self::OtherProblem => None,
        }
    }
}

impl Player {
    pub(super) fn bed_rule_value_allows_in_world(world: &World, value: BedRuleValue) -> bool {
        match value {
            BedRuleValue::Always => true,
            BedRuleValue::WhenDark => world.is_dark_outside(),
            BedRuleValue::Never => false,
        }
    }

    pub(super) fn bed_rule_value_allows(&self, value: BedRuleValue) -> bool {
        Self::bed_rule_value_allows_in_world(&self.get_world(), value)
    }

    fn bed_rule_problem_message(&self) -> Option<TextComponent> {
        self.get_world()
            .dimension_type
            .bed_rule
            .error_message_key
            .as_ref()
            .map(|key| {
                TranslatedMessage {
                    key: (*key).into(),
                    fallback: None,
                    args: None,
                }
                .component()
            })
    }

    fn bed_sleep_problem(&self) -> BedSleepingProblem {
        self.bed_rule_problem_message()
            .map_or(BedSleepingProblem::OtherProblem, |message| {
                BedSleepingProblem::Message(Box::new(message))
            })
    }

    fn is_reachable_bed_block_from_position(player_pos: DVec3, bed_block_pos: BlockPos) -> bool {
        let bed_center = DVec3::new(
            f64::from(bed_block_pos.x()) + 0.5,
            f64::from(bed_block_pos.y()),
            f64::from(bed_block_pos.z()) + 0.5,
        );
        (player_pos.x - bed_center.x).abs() <= BED_INTERACTION_XZ_RANGE
            && (player_pos.y - bed_center.y).abs() <= BED_INTERACTION_Y_RANGE
            && (player_pos.z - bed_center.z).abs() <= BED_INTERACTION_XZ_RANGE
    }

    fn bed_in_range(&self, pos: BlockPos, direction: Direction) -> bool {
        Self::bed_in_range_from_position(self.position(), pos, direction)
    }

    fn bed_in_range_from_position(player_pos: DVec3, pos: BlockPos, direction: Direction) -> bool {
        Self::is_reachable_bed_block_from_position(player_pos, pos)
            || Self::is_reachable_bed_block_from_position(
                player_pos,
                direction.opposite().relative(pos),
            )
    }

    fn bed_blocked(&self, pos: BlockPos, direction: Direction) -> bool {
        Self::bed_blocked_with_free_at(pos, direction, |pos| self.free_at(pos))
    }

    fn bed_blocked_with_free_at(
        pos: BlockPos,
        direction: Direction,
        mut free_at: impl FnMut(BlockPos) -> bool,
    ) -> bool {
        let above = pos.above();
        !free_at(above) || !free_at(direction.opposite().relative(above))
    }

    fn free_at(&self, pos: BlockPos) -> bool {
        !self.get_world().get_block_state(pos).is_suffocating()
    }

    pub(crate) fn stop_sleep_in_bed(&self, forceful_wakeup: bool, update_level_list: bool) {
        if self.is_sleeping() {
            let packet = CAnimate::new(self.id(), AnimateAction::WakeUp);
            self.get_world()
                .broadcast_to_entity_trackers(self.id(), packet.clone(), None);
            self.send_packet(packet);
        }

        self.default_stop_sleeping();
        if update_level_list {
            self.get_world().update_sleeping_player_list();
        }
        self.set_sleep_counter(if forceful_wakeup { 0 } else { SLEEP_DURATION });
        let (yaw, pitch) = self.rotation();
        if let Err(error) = self.teleport(self.position(), yaw, pitch) {
            log::warn!(
                "Failed to teleport player {} after waking up: {error}",
                self.id()
            );
        }
        self.sync_entity_data();
    }

    pub(crate) fn start_sleep_in_bed(&self, pos: BlockPos) -> Result<(), BedSleepingProblem> {
        let world = self.get_world();
        let direction = world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::HORIZONTAL_FACING);
        if self.is_sleeping() || !Entity::is_alive(self) {
            return Err(BedSleepingProblem::OtherProblem);
        }

        let rule = &world.dimension_type.bed_rule;
        let can_sleep = self.bed_rule_value_allows(rule.can_sleep);
        let can_set_spawn = self.bed_rule_value_allows(rule.can_set_spawn);
        if !can_set_spawn && !can_sleep {
            return Err(self.bed_sleep_problem());
        }
        if !self.bed_in_range(pos, direction) {
            return Err(BedSleepingProblem::Message(Box::new(
                TranslatedMessage {
                    key: "block.minecraft.bed.too_far_away".into(),
                    fallback: None,
                    args: None,
                }
                .component(),
            )));
        }
        if self.bed_blocked(pos, direction) {
            return Err(BedSleepingProblem::Message(Box::new(
                TranslatedMessage {
                    key: "block.minecraft.bed.obstructed".into(),
                    fallback: None,
                    args: None,
                }
                .component(),
            )));
        }

        if can_set_spawn {
            self.set_respawn_position(
                Some(PlayerRespawnConfig::new(
                    RespawnData::of(world.key.clone(), pos, self.rotation().0, self.rotation().1),
                    false,
                )),
                true,
            );
        }
        if !can_sleep {
            return Err(self.bed_sleep_problem());
        }

        // TODO: Mirror vanilla Monster::isPreventingPlayerRest once Steel has
        // the required Monster capability/class foundation.
        self.set_sleep_counter(0);
        if self.start_sleeping(pos).is_err() {
            return Err(BedSleepingProblem::OtherProblem);
        }
        self.sync_entity_data();
        self.award_custom_stat(&vanilla_custom_stats::SLEEP_IN_BED);
        // TODO: trigger CriteriaTriggers.SLEPT_IN_BED once the foundation for advancements exist.
        if !world.can_sleep_through_nights() {
            self.send_overlay_message(
                &TranslatedMessage {
                    key: "sleep.not_possible".into(),
                    fallback: None,
                    args: None,
                }
                .component(),
            );
        }
        world.update_sleeping_player_list();
        Ok(())
    }

    /// Returns the player's current vanilla respawn configuration.
    #[must_use]
    pub fn respawn_config(&self) -> Option<PlayerRespawnConfig> {
        self.respawn_config.lock().clone()
    }

    /// Sets the player's vanilla bed or respawn-anchor target.
    pub fn set_respawn_position(
        &self,
        respawn_config: Option<PlayerRespawnConfig>,
        show_message: bool,
    ) {
        let mut current = self.respawn_config.lock();
        if show_message
            && respawn_config
                .as_ref()
                .is_some_and(|config| !config.is_same_position(current.as_ref()))
        {
            self.send_message(
                &TranslatedMessage {
                    key: "block.minecraft.set_spawn".into(),
                    fallback: None,
                    args: None,
                }
                .component(),
            );
        }
        *current = respawn_config;
    }

    /// Returns vanilla `Player.sleepCounter`.
    #[must_use]
    pub fn sleep_counter(&self) -> i32 {
        self.sleep_state.lock().sleep_counter()
    }

    /// Returns whether this player has slept long enough for vanilla night skip.
    #[must_use]
    pub fn is_sleeping_long_enough(&self) -> bool {
        self.is_sleeping() && self.sleep_counter() >= SLEEP_DURATION
    }

    fn set_sleep_counter(&self, sleep_counter: i32) {
        self.sleep_state.lock().set_sleep_counter(sleep_counter);
    }

    pub(super) fn tick_sleep_counter(&self) {
        self.sleep_state
            .lock()
            .tick_sleep_counter(self.is_sleeping());
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_utils::{BlockPos, Direction};

    use super::Player;

    #[test]
    fn sleep_admission_rejects_a_player_outside_vanilla_range() {
        let bed_pos = BlockPos::new(0, 64, 0);
        let direction = Direction::South;

        assert!(Player::bed_in_range_from_position(
            DVec3::new(3.5, 64.0, 0.5),
            bed_pos,
            direction,
        ));
        assert!(!Player::bed_in_range_from_position(
            DVec3::new(4.0, 64.0, 0.5),
            bed_pos,
            direction,
        ));
    }

    #[test]
    fn sleep_admission_checks_both_bed_halves_for_obstruction() {
        let bed_pos = BlockPos::new(0, 64, 0);
        let direction = Direction::South;
        let above_head = bed_pos.above();
        let above_foot = direction.opposite().relative(above_head);

        assert!(!Player::bed_blocked_with_free_at(
            bed_pos,
            direction,
            |_| true,
        ));
        assert!(Player::bed_blocked_with_free_at(
            bed_pos,
            direction,
            |pos| pos != above_head,
        ));
        assert!(Player::bed_blocked_with_free_at(
            bed_pos,
            direction,
            |pos| pos != above_foot,
        ));
    }
}
