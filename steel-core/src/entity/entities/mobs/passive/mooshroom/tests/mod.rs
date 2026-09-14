use std::io::Cursor;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use steel_registry::data_components::components::{SuspiciousStewEffect, SuspiciousStewEffects};
use steel_registry::data_components::vanilla_components::SUSPICIOUS_STEW_EFFECTS;
use steel_registry::{
    REGISTRY, RegistryExt, init_vanilla_registry, vanilla_entities, vanilla_items,
};
use steel_utils::ChunkPos;
use steel_utils::Identifier;
use steel_utils::types::InteractionHand;
use uuid::Uuid;

use crate::entity::{Entity, LivingEntity, Mob, next_entity_id};
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

use super::*;

#[test]
fn mooshroom_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();

    let mooshroom =
        MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 1, DVec3::ZERO, Weak::new());

    assert_eq!(mooshroom.get_health().to_bits(), 10.0_f32.to_bits());
    assert_eq!(mooshroom.variant(), MushroomCowVariant::Red);
    let attributes = mooshroom.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        10.0_f64.to_bits()
    );
}

#[test]
fn mooshroom_milks_bowl_into_mushroom_stew() {
    init_vanilla_registry();

    let world = fresh_test_world("mooshroom_bowl_milking");
    let player = TestPlayerBuilder::new(world, "Milker", 10).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BOWL));

    let mooshroom =
        MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 1, DVec3::ZERO, Weak::new());

    assert_eq!(
        Mob::mob_interact(&mooshroom, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .is(&vanilla_items::MUSHROOM_STEW)
    );
}

#[test]
fn mooshroom_milks_bucket_into_milk_bucket() {
    init_vanilla_registry();

    let world = fresh_test_world("mooshroom_bucket_milking");
    let player = TestPlayerBuilder::new(world, "Milker", 11).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BUCKET));

    let mooshroom =
        MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 1, DVec3::ZERO, Weak::new());

    assert_eq!(
        Mob::mob_interact(&mooshroom, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .is(&vanilla_items::MILK_BUCKET)
    );
}

#[test]
fn brown_mooshroom_eats_flower_and_gives_suspicious_stew() {
    init_vanilla_registry();

    let world = fresh_test_world("mooshroom_suspicious_stew");
    let player = TestPlayerBuilder::new(world, "FlowerFeeder", 12).build();

    let mooshroom =
        MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 1, DVec3::ZERO, Weak::new());
    mooshroom.set_variant(MushroomCowVariant::Brown);

    let dandelion = ItemStack::new(&vanilla_items::DANDELION);
    player.inventory.lock().set_selected_item(dandelion);

    assert_eq!(
        Mob::mob_interact(&mooshroom, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::SuccessServer
    );
    assert!(mooshroom.stew_effects().is_some());

    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BOWL));
    assert_eq!(
        Mob::mob_interact(&mooshroom, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Success
    );

    {
        let inventory = player.inventory.lock();
        let stew = inventory.get_selected_item();
        assert!(stew.is(&vanilla_items::SUSPICIOUS_STEW));
        assert!(stew.get(SUSPICIOUS_STEW_EFFECTS).is_some());
    }

    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BOWL));
    assert_eq!(
        Mob::mob_interact(&mooshroom, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .is(&vanilla_items::MUSHROOM_STEW)
    );
}

#[test]
fn mooshroom_thunder_hit_swaps_variant() {
    init_vanilla_registry();

    let mooshroom =
        MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 1, DVec3::ZERO, Weak::new());
    assert_eq!(mooshroom.variant(), MushroomCowVariant::Red);

    let bolt1 = Uuid::new_v4();
    mooshroom.thunder_hit(bolt1);
    assert_eq!(mooshroom.variant(), MushroomCowVariant::Brown);

    mooshroom.thunder_hit(bolt1);
    assert_eq!(mooshroom.variant(), MushroomCowVariant::Brown);

    let bolt2 = Uuid::new_v4();
    mooshroom.thunder_hit(bolt2);
    assert_eq!(mooshroom.variant(), MushroomCowVariant::Red);
}

#[test]
fn mooshroom_shearing_converts_to_cow_and_drops_mushrooms() {
    init_vanilla_registry();

    let world = fresh_test_world("mooshroom_shearing");
    let player = TestPlayerBuilder::new(world.clone(), "Shearer", 13).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::SHEARS));

    let mooshroom = Arc::new(MushroomCowEntity::new(
        &vanilla_entities::MOOSHROOM,
        next_entity_id(),
        DVec3::new(10.0, 64.0, 10.0),
        Arc::downgrade(&world),
    ));
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(mooshroom.block_position()));
    world
        .try_add_entity(mooshroom.clone())
        .expect("mooshroom added");

    assert_eq!(
        Mob::mob_interact(
            mooshroom.as_ref(),
            player.as_ref(),
            InteractionHand::MainHand
        ),
        InteractionResult::SuccessServer
    );

    assert!(mooshroom.is_removed());
}

#[test]
fn mooshroom_nbt_persistence_roundtrip() {
    init_vanilla_registry();

    let mooshroom =
        MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 1, DVec3::ZERO, Weak::new());
    mooshroom.set_variant(MushroomCowVariant::Brown);

    let night_vision = REGISTRY
        .mob_effects
        .by_key(&Identifier::vanilla_static("night_vision"))
        .expect("night vision should exist");
    let effects = SuspiciousStewEffects::new(vec![SuspiciousStewEffect::new(night_vision, 200)]);
    mooshroom.set_stew_effects(Some(effects.clone()));

    let mut nbt = NbtCompound::new();
    mooshroom.save_additional(&mut nbt);

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);

    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let loaded = MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 2, DVec3::ZERO, Weak::new());
    loaded.load_additional((&borrowed).into());

    assert_eq!(loaded.variant(), MushroomCowVariant::Brown);
    assert_eq!(loaded.stew_effects(), Some(effects));
}
