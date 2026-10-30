//! Exact `CombatTracker` port used for death messages and combat status.

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::damage_type::DeathMessageType;
use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_damage_type_tags;
use steel_utils::BlockStateId;
use steel_utils::translations::CHAT_SQUARE_BRACKETS;
use text_components::interactivity::{ClickEvent, HoverEvent};
use text_components::{Modifier as _, TextComponent};

use super::DamageSource;
use super::source::translated_message;
use crate::behavior::ITEM_BEHAVIORS;
use crate::entity::{LivingEntity, SharedEntity};
use crate::inventory::equipment::EquipmentSlot;

/// Ticks after the last hit before damage status resets outside combat.
const RESET_DAMAGE_STATUS_TIME: i32 = 100;
/// Ticks after the last hit before combat status resets.
const RESET_COMBAT_STATUS_TIME: i32 = 300;
const INTENTIONAL_GAME_DESIGN_BUG: &str = "https://bugs.mojang.com/browse/MCPE-28723";
const INTENTIONAL_GAME_DESIGN_BUG_ID: &str = "MCPE-28723";

/// A single recorded hit.
#[derive(Debug, Clone)]
pub struct CombatEntry {
    /// The damage source that produced this entry.
    pub source: DamageSource,
    /// Damage dealt after absorption.
    pub damage: f32,
    /// Where the victim was climbing or swimming when hit.
    pub fall_location: Option<FallLocation>,
    /// The victim's fall distance when hit.
    pub fall_distance: f32,
}

/// Where a falling victim was climbing or swimming, used to pick the accident message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallLocation {
    /// Not climbing.
    Generic,
    /// Ladders and trapdoors.
    Ladder,
    /// Vines.
    Vines,
    /// Weeping vines and their plant segments.
    WeepingVines,
    /// Twisting vines and their plant segments.
    TwistingVines,
    /// Scaffolding.
    Scaffolding,
    /// Any other climbable block.
    OtherClimbable,
    /// Falling in water.
    Water,
}

impl FallLocation {
    /// Categorizes a block state the victim was climbing.
    #[must_use]
    pub fn from_block_state(state: BlockStateId) -> Self {
        let block = state.get_block();
        if block == &vanilla_blocks::LADDER || block.has_tag(&BlockTag::TRAPDOORS) {
            Self::Ladder
        } else if block == &vanilla_blocks::VINE {
            Self::Vines
        } else if block == &vanilla_blocks::WEEPING_VINES
            || block == &vanilla_blocks::WEEPING_VINES_PLANT
        {
            Self::WeepingVines
        } else if block == &vanilla_blocks::TWISTING_VINES
            || block == &vanilla_blocks::TWISTING_VINES_PLANT
        {
            Self::TwistingVines
        } else if block == &vanilla_blocks::SCAFFOLDING {
            Self::Scaffolding
        } else {
            Self::OtherClimbable
        }
    }

    /// Returns the `death.fell.accident.*` key for this location.
    #[must_use]
    pub const fn language_key(self) -> &'static str {
        match self {
            Self::Generic => "death.fell.accident.generic",
            Self::Ladder => "death.fell.accident.ladder",
            Self::Vines => "death.fell.accident.vines",
            Self::WeepingVines => "death.fell.accident.weeping_vines",
            Self::TwistingVines => "death.fell.accident.twisting_vines",
            Self::Scaffolding => "death.fell.accident.scaffolding",
            Self::OtherClimbable => "death.fell.accident.other_climbable",
            Self::Water => "death.fell.accident.water",
        }
    }
}

/// Per-entity combat state.
#[derive(Debug, Clone)]
pub(crate) struct CombatTracker {
    entries: Vec<CombatEntry>,
    last_damage_time: i32,
    combat_start_time: i32,
    combat_end_time: i32,
    in_combat: bool,
    taking_damage: bool,
}

impl CombatTracker {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_damage_time: 0,
            combat_start_time: 0,
            combat_end_time: 0,
            in_combat: false,
            taking_damage: false,
        }
    }

    /// Records a hit and enters damage-taking state.
    pub(crate) fn record_damage(
        &mut self,
        source: &DamageSource,
        damage: f32,
        fall_location: Option<FallLocation>,
        fall_distance: f32,
        tick_count: i32,
    ) {
        self.entries.push(CombatEntry {
            source: source.clone(),
            damage,
            fall_location,
            fall_distance,
        });
        self.last_damage_time = tick_count;
        self.taking_damage = true;
    }

    /// Enters combat, returning whether the state changed.
    pub(crate) const fn enter_combat(&mut self, tick_count: i32) -> bool {
        if self.in_combat {
            return false;
        }
        self.in_combat = true;
        self.combat_start_time = tick_count;
        self.combat_end_time = tick_count;
        true
    }

    /// Resets expired combat state, returning whether combat ended.
    pub(crate) fn recheck_status(&mut self, tick_count: i32, alive: bool) -> bool {
        let reset = if self.in_combat {
            RESET_COMBAT_STATUS_TIME
        } else {
            RESET_DAMAGE_STATUS_TIME
        };
        let expired = !alive || tick_count - self.last_damage_time > reset;
        if !self.taking_damage || !expired {
            return false;
        }

        let was_in_combat = self.in_combat;
        self.taking_damage = false;
        self.in_combat = false;
        self.combat_end_time = tick_count;
        self.entries.clear();
        was_in_combat
    }

    /// Returns the combat duration in ticks.
    #[must_use]
    pub(crate) const fn duration(&self, tick_count: i32) -> i32 {
        if self.in_combat {
            tick_count - self.combat_start_time
        } else {
            self.combat_end_time - self.combat_start_time
        }
    }

    pub(crate) fn entries(&self) -> &[CombatEntry] {
        &self.entries
    }

    /// Drops recorded hits without changing combat status.
    pub(crate) fn clear_entries(&mut self) {
        self.entries.clear();
    }
}

/// Builds the death message from the recorded combat entries.
pub(crate) fn death_message<E: LivingEntity + ?Sized>(victim: &E) -> TextComponent {
    let entries = victim.living_base().combat_entries();
    let Some(killing_blow) = entries.last() else {
        return translated_message(
            "death.attack.generic".to_owned(),
            vec![victim.display_name()],
        );
    };
    let killing_source = &killing_blow.source;

    match killing_source.damage_type.death_message_type {
        DeathMessageType::FallVariants => {
            if let Some(knock_off_entry) = most_significant_fall(&entries) {
                return fall_message(victim, knock_off_entry, killing_source.causing_entity());
            }
        }
        DeathMessageType::IntentionalGameDesign => {
            let death_message_key =
                format!("death.attack.{}", killing_source.damage_type.message_id);
            let link =
                TextComponent::translated(CHAT_SQUARE_BRACKETS.message([translated_message(
                    format!("{death_message_key}.link"),
                    Vec::new(),
                )]))
                .click_event(ClickEvent::open_url(INTENTIONAL_GAME_DESIGN_BUG))
                .hover_event(HoverEvent::show_text(TextComponent::plain(
                    INTENTIONAL_GAME_DESIGN_BUG_ID,
                )));
            return translated_message(
                format!("{death_message_key}.message"),
                vec![victim.display_name(), link],
            );
        }
        DeathMessageType::Default => {}
    }

    killing_source.get_localized_death_message(victim)
}

/// Picks the entry describing the worst fall, if any.
fn most_significant_fall(entries: &[CombatEntry]) -> Option<&CombatEntry> {
    let mut result = None;
    let mut alternative = None;
    let mut alternative_damage = 0.0;
    let mut best_fall = 0.0;

    for (index, entry) in entries.iter().enumerate() {
        let previous = index.checked_sub(1).map(|previous| &entries[previous]);
        let is_fake_fall = entry
            .source
            .is(&vanilla_damage_type_tags::DamageTypeTag::ALWAYS_MOST_SIGNIFICANT_FALL);
        let fall_distance = if is_fake_fall {
            f32::MAX
        } else {
            entry.fall_distance
        };

        if (entry
            .source
            .is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
            || is_fake_fall)
            && fall_distance > 0.0
            && (result.is_none() || fall_distance > best_fall)
        {
            result = if index > 0 { previous } else { Some(entry) };
            best_fall = fall_distance;
        }

        if entry.fall_location.is_some()
            && (alternative.is_none() || entry.damage > alternative_damage)
        {
            alternative = Some(entry);
            alternative_damage = entry.damage;
        }
    }

    if best_fall > 5.0 {
        result
    } else if alternative_damage > 5.0 {
        alternative
    } else {
        None
    }
}

/// Builds a fall death message from the entry that knocked the victim off.
fn fall_message<E: LivingEntity + ?Sized>(
    victim: &E,
    knock_off_entry: &CombatEntry,
    killing_entity: Option<&SharedEntity>,
) -> TextComponent {
    let knock_off_source = &knock_off_entry.source;
    if !knock_off_source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
        && !knock_off_source
            .is(&vanilla_damage_type_tags::DamageTypeTag::ALWAYS_MOST_SIGNIFICANT_FALL)
    {
        let killer_name = killing_entity.map(|entity| entity.display_name());
        let attacker_entity = knock_off_source.causing_entity();
        let attacker_name = attacker_entity.map(|entity| entity.display_name());

        if let Some(attacker_name) = attacker_name.as_ref()
            && killer_name.as_ref() != Some(attacker_name)
        {
            return assisted_fall_message(
                victim,
                attacker_entity,
                attacker_name.clone(),
                "death.fell.assist.item",
                "death.fell.assist",
            );
        }

        return if let Some(killer_name) = killer_name {
            assisted_fall_message(
                victim,
                killing_entity,
                killer_name,
                "death.fell.finish.item",
                "death.fell.finish",
            )
        } else {
            translated_message("death.fell.killer".to_owned(), vec![victim.display_name()])
        };
    }

    let fall_location = knock_off_entry
        .fall_location
        .unwrap_or(FallLocation::Generic);
    translated_message(
        fall_location.language_key().to_owned(),
        vec![victim.display_name()],
    )
}

/// Builds a fall message naming `attacker_entity`, including their named item.
fn assisted_fall_message<E: LivingEntity + ?Sized>(
    victim: &E,
    attacker_entity: Option<&SharedEntity>,
    attacker_name: TextComponent,
    message_with_item: &str,
    message_without_item: &str,
) -> TextComponent {
    let attacker_item = attacker_entity
        .and_then(|entity| entity.as_living_entity())
        .map(|living| {
            living
                .living_base()
                .equipment()
                .lock()
                .get_ref(EquipmentSlot::MainHand)
                .clone()
        })
        .filter(|item| !item.is_empty());

    match attacker_item {
        Some(item) if item.has(CUSTOM_NAME) => translated_message(
            message_with_item.to_owned(),
            vec![
                victim.display_name(),
                attacker_name,
                ITEM_BEHAVIORS.display_name(&item),
            ],
        ),
        _ => translated_message(
            message_without_item.to_owned(),
            vec![victim.display_name(), attacker_name],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::damage_type::DamageType;
    use steel_registry::{init_vanilla_registry, vanilla_damage_types};

    fn entry(
        damage_type: &'static DamageType,
        damage: f32,
        fall_distance: f32,
        fall_location: Option<FallLocation>,
    ) -> CombatEntry {
        CombatEntry {
            source: DamageSource::environment(damage_type),
            damage,
            fall_location,
            fall_distance,
        }
    }

    #[test]
    fn recheck_status_resets_damage_status_after_its_window() {
        let mut tracker = CombatTracker::new();
        tracker.record_damage(
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            1.0,
            None,
            0.0,
            0,
        );

        assert!(!tracker.recheck_status(100, true));
        assert_eq!(tracker.entries().len(), 1);

        assert!(!tracker.recheck_status(101, true));
        assert!(tracker.entries().is_empty());
    }

    #[test]
    fn recheck_status_uses_the_longer_window_in_combat() {
        let mut tracker = CombatTracker::new();
        assert!(tracker.enter_combat(0));
        tracker.record_damage(
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            1.0,
            None,
            0.0,
            0,
        );

        assert!(!tracker.recheck_status(300, true));
        assert!(tracker.recheck_status(301, true));
    }

    #[test]
    fn most_significant_fall_attributes_a_lethal_fall_to_the_preceding_hit() {
        init_vanilla_registry();
        let entries = [
            entry(&vanilla_damage_types::MOB_ATTACK, 1.0, 0.0, None),
            entry(&vanilla_damage_types::FALL, 5.0, 10.0, None),
        ];

        let selected = most_significant_fall(&entries).expect("a 10 block fall is significant");

        assert_eq!(selected.damage, 1.0);
    }

    #[test]
    fn most_significant_fall_falls_back_to_heavy_damage_while_climbing() {
        init_vanilla_registry();
        let entries = [
            entry(
                &vanilla_damage_types::MOB_ATTACK,
                6.0,
                0.0,
                Some(FallLocation::Ladder),
            ),
            entry(&vanilla_damage_types::GENERIC, 1.0, 0.0, None),
        ];

        let selected =
            most_significant_fall(&entries).expect("climbing damage above 5 should qualify");

        assert_eq!(selected.fall_location, Some(FallLocation::Ladder));
    }
}
