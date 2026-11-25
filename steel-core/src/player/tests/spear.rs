//! Spear jab and kinetic charge attacks.

use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::{PlayerAction, SPlayerAction, SSetCarriedItem};
use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;
use steel_utils::BlockPos;
use steel_utils::types::InteractionHand;

use super::MeleeFixture;
use crate::entity::entities::PigEntity;
use crate::entity::{Entity as _, LivingEntity as _, SharedEntity};
use crate::inventory::container::Container as _;
use crate::player::Player;

/// Faces the attacker along +X with a level look, at a pig floating at eye height
/// just past the spear's 2-block minimum reach.
fn spear_fixture(name: &'static str) -> (MeleeFixture, Arc<PigEntity>) {
    const FACE_POSITIVE_X_YAW: f32 = -90.0;
    let fixture = MeleeFixture::new(name);
    fixture.attacker.set_rotation((FACE_POSITIVE_X_YAW, 0.0));
    // Piercing rays follow head yaw; charges read last tick's head yaw.
    fixture.attacker.set_y_head_rot(FACE_POSITIVE_X_YAW);
    fixture.attacker.advance_living_rotation_for_base_tick();
    fixture
        .attacker
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::NETHERITE_SPEAR));
    let pig = fixture.spawn_pig(DVec3::new(2.5, 1.0, 0.0));
    (fixture, pig)
}

#[test]
fn spear_jab_hits_targets_outside_invulnerability_frames() {
    let (fixture, pig) = spear_fixture("player_spear_jab");
    fixture.charge_attack();
    fixture.attacker.handle_player_action(SPlayerAction {
        action: PlayerAction::Stab,
        pos: BlockPos::ZERO,
        direction: Direction::Down,
        sequence: 0,
    });
    assert!(pig.get_health() < pig.get_max_health());
}

#[test]
fn spear_charge_damages_by_relative_speed_after_delay_and_respects_contact_cooldown() {
    // 0.25 blocks/tick is 5 blocks/s: above the netherite spear's 4.6 relative-speed
    // damage threshold, below its 5.1 knockback threshold.
    const APPROACH_BLOCKS_PER_TICK: f64 = 0.25;
    // Player base attack damage 1 + floor(5 blocks/s * 1.2 damage multiplier).
    const CHARGE_DAMAGE: f32 = 7.0;
    const NETHERITE_SPEAR_DELAY_TICKS: usize = 8;

    let (fixture, pig) = spear_fixture("player_spear_charge");
    let attacker = &fixture.attacker;
    let attacker_entity: SharedEntity = Arc::<Player>::clone(attacker);
    attacker
        .movement
        .lock()
        .set_last_known_client_movement(DVec3::new(APPROACH_BLOCKS_PER_TICK, 0.0, 0.0));
    let start_health = pig.get_health();

    attacker.start_using_item(InteractionHand::MainHand);
    for _ in 0..NETHERITE_SPEAR_DELAY_TICKS {
        attacker.tick_active_item_use(&attacker_entity);
    }
    assert!((pig.get_health() - start_health).abs() < f32::EPSILON);

    attacker.tick_active_item_use(&attacker_entity);
    assert!((pig.get_health() - (start_health - CHARGE_DAMAGE)).abs() < f32::EPSILON);
    assert!(
        attacker
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .get_damage_value()
            > 0,
        "the charge should wear the spear"
    );

    // Within the 10-tick contact cooldown the same pig is not stabbed again, even
    // once its hurt invulnerability is cleared.
    pig.set_invulnerable_time(0);
    attacker.tick_active_item_use(&attacker_entity);
    assert!((pig.get_health() - (start_health - CHARGE_DAMAGE)).abs() < f32::EPSILON);
    assert!(attacker.is_using_item());
}

#[test]
fn switching_to_an_identical_spear_restarts_the_charge_windup() {
    const OTHER_HOTBAR_SLOT: i16 = 1;

    let (fixture, _pig) = spear_fixture("player_spear_slot_switch");
    let attacker = &fixture.attacker;
    attacker.inventory.lock().set_item(
        OTHER_HOTBAR_SLOT as usize,
        ItemStack::new(&vanilla_items::NETHERITE_SPEAR),
    );

    attacker.start_using_item(InteractionHand::MainHand);
    attacker.handle_set_carried_item(SSetCarriedItem {
        slot: OTHER_HOTBAR_SLOT,
    });
    assert!(!attacker.is_using_item());
}
