//! NBT round-trips for the three keys the dragon persists.
//!
//! Every test that saves first would hide a load-side bug behind a matching save-side
//! one, so the load direction is driven from hand-built NBT wherever the value can be
//! written by hand.

use super::*;

/// Loads `nbt` into a fresh world-less dragon and hands it back.
///
/// `load_additional` takes a borrowed compound, so hand-built NBT has to go out to
/// bytes and come back before it can be loaded.
fn load_into_fresh_dragon(nbt: &NbtCompound) -> EnderDragonEntity {
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let parsed = read_borrowed_compound(&mut Cursor::new(&bytes[..]))
        .expect("the test dragon nbt should parse");
    let dragon = test_dragon();
    dragon.load_additional((&parsed).into());
    dragon
}

#[test]
fn a_fresh_dragon_saves_its_defaults() {
    let dragon = test_dragon();
    assert_eq!(dragon.dragon_death_time(), 0);

    let mut nbt = NbtCompound::new();
    dragon.save_additional(&mut nbt);

    assert_eq!(nbt.int("DragonDeathTime"), Some(0));
    assert_eq!(nbt.float("sitting_damage_received"), Some(0.0));
    assert_eq!(
        nbt.int("DragonPhase"),
        Some(EnderDragonPhase::Hovering.id())
    );
}

#[test]
fn the_phase_round_trips_through_nbt() {
    let dragon = test_dragon();
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::HoldingPattern);

    let mut nbt = NbtCompound::new();
    dragon.save_additional(&mut nbt);
    assert_eq!(
        nbt.int("DragonPhase"),
        Some(EnderDragonPhase::HoldingPattern.id())
    );

    let restored = load_into_fresh_dragon(&nbt);
    assert_eq!(
        restored.phase_manager().current_phase(),
        EnderDragonPhase::HoldingPattern
    );
}

#[test]
fn the_phase_loads_from_hand_written_nbt() {
    let mut nbt = NbtCompound::new();
    nbt.insert("DragonPhase", EnderDragonPhase::Dying.id());

    let dragon = load_into_fresh_dragon(&nbt);

    assert_eq!(
        dragon.phase_manager().current_phase(),
        EnderDragonPhase::Dying
    );
}

#[test]
fn the_death_timer_loads_from_hand_written_nbt() {
    let mut nbt = NbtCompound::new();
    nbt.insert("DragonDeathTime", 137_i32);

    let dragon = load_into_fresh_dragon(&nbt);

    assert_eq!(dragon.dragon_death_time(), 137);
}

#[test]
fn the_death_timer_round_trips_through_nbt_mid_animation() {
    let world = death_test_world("dragon_persistence_death_timer");
    let dragon = dragon_at(&world, DVec3::new(0.5, 80.0, 0.5));
    for _ in 0..37 {
        dragon.tick_death();
    }

    let mut nbt = NbtCompound::new();
    dragon.save_additional(&mut nbt);
    assert_eq!(nbt.int("DragonDeathTime"), Some(37));

    let restored = load_into_fresh_dragon(&nbt);
    assert_eq!(restored.dragon_death_time(), 37);
}

#[test]
fn the_sitting_damage_counter_round_trips_through_nbt() {
    let mut nbt = NbtCompound::new();
    nbt.insert("sitting_damage_received", 42.5_f32);

    let dragon = load_into_fresh_dragon(&nbt);

    // No getter exposes the counter, so the value is observed by saving it back out.
    let mut resaved = NbtCompound::new();
    dragon.save_additional(&mut resaved);
    assert_eq!(resaved.float("sitting_damage_received"), Some(42.5));
}

#[test]
fn absent_keys_leave_the_dragon_at_its_defaults() {
    // Every key in `load_additional` is guarded by `if let Some(..)`, so an NBT tag
    // written by an older version must leave the dragon exactly as constructed.
    let dragon = load_into_fresh_dragon(&NbtCompound::new());

    assert_eq!(dragon.dragon_death_time(), 0);
    assert_eq!(
        dragon.phase_manager().current_phase(),
        EnderDragonPhase::Hovering
    );

    let mut resaved = NbtCompound::new();
    dragon.save_additional(&mut resaved);
    assert_eq!(resaved.float("sitting_damage_received"), Some(0.0));
}
