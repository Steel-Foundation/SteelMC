use std::io::Cursor;

use simdnbt::borrow::read_compound as read_borrowed_compound;
use steel_registry::{init_vanilla_registry, vanilla_attributes, vanilla_entities, vanilla_items};

use crate::behavior::init_behaviors;
use crate::entity::SharedEntity;
use crate::entity::ai::goal::Goal;
use crate::entity::entities::PigEntity;
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

use super::*;

fn new_fox() -> FoxEntity {
    FoxEntity::new(&vanilla_entities::FOX, 1, DVec3::ZERO, Weak::new())
}

fn world_with_fox(name: &'static str) -> (Arc<World>, Arc<FoxEntity>) {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world(name);
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let fox = Arc::new(FoxEntity::new(
        &vanilla_entities::FOX,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    ));
    world
        .try_add_entity(Arc::clone(&fox) as SharedEntity)
        .expect("fox should attach to the loaded chunk");
    (world, fox)
}

fn add_item(world: &Arc<World>, item: ItemStack) -> Arc<ItemEntity> {
    let entity = Arc::new(ItemEntity::with_item(
        &vanilla_entities::ITEM,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        item,
        Arc::downgrade(world),
    ));
    entity.set_no_pickup_delay();
    world
        .try_add_entity(Arc::clone(&entity) as SharedEntity)
        .expect("item should attach to the loaded chunk");
    entity
}

#[test]
fn fox_starts_red_and_picks_up_loot() {
    init_vanilla_registry();

    let fox = new_fox();

    assert_eq!(fox.variant(), FoxVariant::Red);
    assert!(
        Mob::can_pick_up_loot(&fox),
        "vanilla foxes have canPickUpLoot enabled"
    );
    assert_eq!(fox.get_health().to_bits(), fox.get_max_health().to_bits());
    let attributes = fox.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        10.0_f64.to_bits()
    );
}

#[test]
fn fox_variant_round_trips() {
    init_vanilla_registry();

    let fox = new_fox();

    fox.set_variant(FoxVariant::Snow);
    assert_eq!(fox.variant(), FoxVariant::Snow);

    fox.set_variant(FoxVariant::Red);
    assert_eq!(fox.variant(), FoxVariant::Red);
}

#[test]
fn fox_flags_are_independent_bits() {
    init_vanilla_registry();

    let fox = new_fox();

    fox.set_sitting(true);
    fox.set_crouching(true);
    assert!(fox.is_sitting());
    assert!(fox.is_crouching());
    assert!(!fox.is_sleeping());

    // Clearing one flag must not disturb the others sharing the byte.
    fox.set_sitting(false);
    assert!(!fox.is_sitting());
    assert!(fox.is_crouching());
}

#[test]
fn fox_uses_vanilla_fox_food_tag() {
    init_vanilla_registry();

    assert!(FoxEntity::is_food(&ItemStack::new(
        &vanilla_items::SWEET_BERRIES
    )));
    assert!(!FoxEntity::is_food(&ItemStack::new(&vanilla_items::STONE)));
}

#[test]
fn fox_saves_and_loads_variant_and_state_flags() {
    init_vanilla_registry();

    let fox = new_fox();
    fox.set_variant(FoxVariant::Snow);
    fox.set_sleeping(true);
    fox.set_sitting(true);
    fox.set_crouching(true);

    let mut nbt = NbtCompound::new();
    fox.save_additional(&mut nbt);
    assert_eq!(nbt.byte("Sleeping"), Some(1));
    assert_eq!(nbt.byte("Sitting"), Some(1));
    assert_eq!(nbt.byte("Crouching"), Some(1));

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let loaded = new_fox();
    loaded.load_additional((&borrowed).into());

    assert_eq!(loaded.variant(), FoxVariant::Snow);
    assert!(loaded.is_sleeping());
    assert!(loaded.is_sitting());
    assert!(loaded.is_crouching());
}

#[test]
fn fox_can_hold_item_follows_vanilla_swap_rules() {
    init_vanilla_registry();

    let fox = new_fox();
    let berries = ItemStack::new(&vanilla_items::SWEET_BERRIES);
    let stone = ItemStack::new(&vanilla_items::STONE);

    // Empty mouth: the fox will hold anything.
    assert!(Mob::can_hold_item(&fox, &stone));

    // Holding a non-food item, once feeding has started, it swaps for food only.
    fox.living_base()
        .equipment()
        .lock()
        .set(EquipmentSlot::MainHand, stone.clone());
    *fox.ticks_since_eaten.lock() = 5;
    assert!(
        Mob::can_hold_item(&fox, &berries),
        "a non-food item is swapped for food"
    );
    assert!(
        !Mob::can_hold_item(&fox, &stone),
        "a non-food item is not swapped for another non-food item"
    );

    // Holding food, it will not swap for more food.
    fox.living_base()
        .equipment()
        .lock()
        .set(EquipmentSlot::MainHand, berries.clone());
    assert!(!Mob::can_hold_item(&fox, &berries));
}

#[test]
fn fox_takes_a_nearby_item_into_its_mouth() {
    let (world, fox) = world_with_fox("fox_pickup");
    let item = add_item(&world, ItemStack::new(&vanilla_items::EMERALD));

    Mob::tick_looting(fox.as_ref());

    assert!(item.is_removed(), "the picked-up item entity is discarded");
    let mut holds_emerald = false;
    fox.with_equipment_slot(EquipmentSlot::MainHand, &mut |held| {
        holds_emerald = held.is(&vanilla_items::EMERALD);
    });
    assert!(holds_emerald, "the fox holds the item in its mouth");
    assert!(fox.is_equipment_drop_preserved(EquipmentSlot::MainHand));
    assert_eq!(*fox.ticks_since_eaten.lock(), 0);
}

#[test]
fn fox_spits_out_its_current_item_when_grabbing_another() {
    let (world, fox) = world_with_fox("fox_spit");
    fox.living_base().equipment().lock().set(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::STONE),
    );
    // Feeding must have started for a fox to swap a held non-food item for food.
    *fox.ticks_since_eaten.lock() = 5;
    let item = add_item(&world, ItemStack::new(&vanilla_items::SWEET_BERRIES));

    Mob::pick_up_item(fox.as_ref(), &world, &item);

    let mut holds_berries = false;
    fox.with_equipment_slot(EquipmentSlot::MainHand, &mut |held| {
        holds_berries = held.is(&vanilla_items::SWEET_BERRIES);
    });
    assert!(holds_berries, "the fox now holds the new food item");

    let search = fox.bounding_box().inflate(4.0);
    let spat_stone = world
        .get_entities_in_aabb(&search)
        .into_iter()
        .filter_map(|entity| {
            entity
                .downcast_ref::<ItemEntity>()
                .map(ItemEntity::get_item)
        })
        .any(|stack| stack.is(&vanilla_items::STONE));
    assert!(
        spat_stone,
        "the stone the fox was holding is spat back into the world"
    );
}

#[test]
fn fox_saves_and_loads_trusted_players() {
    init_vanilla_registry();

    let fox = new_fox();
    let first = Uuid::from_u128(0x1234_5678);
    let second = Uuid::from_u128(0x9abc_def0);
    fox.add_trusted(first);
    fox.add_trusted(second);

    let mut nbt = NbtCompound::new();
    fox.save_additional(&mut nbt);

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let loaded = new_fox();
    loaded.load_additional((&borrowed).into());

    assert!(loaded.trusts(first));
    assert!(loaded.trusts(second));
}

#[test]
fn fox_spawn_held_item_is_always_a_vanilla_candidate() {
    init_vanilla_registry();

    let allowed = [
        &vanilla_items::EMERALD,
        &vanilla_items::EGG,
        &vanilla_items::RABBIT_FOOT,
        &vanilla_items::RABBIT_HIDE,
        &vanilla_items::WHEAT,
        &vanilla_items::LEATHER,
        &vanilla_items::FEATHER,
    ];

    for _ in 0..64 {
        let held = FoxEntity::spawn_held_item();
        assert!(
            allowed.iter().any(|item| held.is(item)),
            "spawn held item should be one of the vanilla candidates"
        );
    }
}

#[test]
fn fox_does_not_search_for_items_with_a_full_mouth() {
    let (world, fox) = world_with_fox("fox_search_full");
    fox.living_base().equipment().lock().set(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::EMERALD),
    );
    let _item = add_item(&world, ItemStack::new(&vanilla_items::WHEAT));

    let mut goal = FoxSearchForItemsGoal;
    assert!(
        !goal.can_use(fox.as_ref()),
        "a fox with a full mouth does not search for items"
    );
}

#[test]
fn fox_sleep_goal_stays_usable_while_sleeping() {
    let (_world, fox) = world_with_fox("fox_sleep");
    fox.set_sleeping(true);

    let mut goal = FoxSleepGoal::new();
    assert!(
        goal.can_use(fox.as_ref()),
        "a still, already-sleeping fox keeps the sleep goal active"
    );
}

#[test]
fn fox_is_alertable_to_a_nearby_untrusted_entity() {
    let (world, fox) = world_with_fox("fox_alertable");

    // A lone fox has nothing to be wary of.
    assert!(!fox.is_alertable(), "a fox alone is not alertable");

    // A nearby awake, non-sneaking entity makes the fox wary (vanilla's else-branch).
    let pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        DVec3::new(9.0, 65.0, 8.0),
        Arc::downgrade(&world),
    ));
    world
        .try_add_entity(Arc::clone(&pig) as SharedEntity)
        .expect("pig should attach to the loaded chunk");
    assert!(
        fox.is_alertable(),
        "a nearby untrusted entity makes a fox alertable"
    );

    // A trusted entity is ignored.
    fox.add_trusted(pig.uuid());
    assert!(
        !fox.is_alertable(),
        "a trusted entity does not alert the fox"
    );
}

#[test]
fn fox_kit_inherits_a_parent_variant() {
    init_vanilla_registry();

    let parent = new_fox();
    let partner = new_fox();
    parent.set_variant(FoxVariant::Snow);
    partner.set_variant(FoxVariant::Snow);

    let offspring = new_fox();
    parent.initialize_breed_offspring(&partner, &offspring);

    // Both parents are snow foxes, so the random pick is snow either way.
    assert_eq!(offspring.variant(), FoxVariant::Snow);
}

#[test]
fn fox_kit_trusts_both_parents_love_cause_players() {
    init_vanilla_registry();

    let parent = new_fox();
    let partner = new_fox();
    let fed_parent = Uuid::from_u128(0xa11ce);
    let fed_partner = Uuid::from_u128(0xb0b);
    parent.set_love_cause_uuid(Some(fed_parent));
    partner.set_love_cause_uuid(Some(fed_partner));

    let offspring = new_fox();
    parent.initialize_breed_offspring(&partner, &offspring);

    assert!(offspring.trusts(fed_parent));
    assert!(offspring.trusts(fed_partner));
}

#[test]
fn fox_kit_trusts_the_only_feeding_player() {
    init_vanilla_registry();

    let parent = new_fox();
    let partner = new_fox();
    let feeder = Uuid::from_u128(0xfeed);
    // Only one parent was bred by a player.
    partner.set_love_cause_uuid(Some(feeder));

    let offspring = new_fox();
    parent.initialize_breed_offspring(&partner, &offspring);

    assert!(offspring.trusts(feeder));
}

#[test]
fn fox_drops_its_mouth_item_on_death_regardless_of_loot_rules() {
    let (world, fox) = world_with_fox("fox_death_drop");
    fox.living_base().equipment().lock().set(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::SWEET_BERRIES),
    );

    // The unconditional death-equipment drop runs before the loot-rules gate.
    LivingEntity::drop_custom_death_equipment(fox.as_ref(), &world);

    // The mouth is emptied...
    let mut mouth_empty = false;
    fox.with_equipment_slot(EquipmentSlot::MainHand, &mut |held| {
        mouth_empty = held.is_empty();
    });
    assert!(mouth_empty, "the fox drops the held mouth item on death");

    // ...and the berry is now a dropped item in the world.
    let search = fox.bounding_box().inflate(4.0);
    let dropped = world
        .get_entities_in_aabb(&search)
        .into_iter()
        .filter_map(|entity| {
            entity
                .downcast_ref::<ItemEntity>()
                .map(ItemEntity::get_item)
        })
        .any(|stack| stack.is(&vanilla_items::SWEET_BERRIES));
    assert!(dropped, "the mouth item is dropped into the world");
}

#[test]
fn fox_move_and_look_controls_gate_on_state() {
    init_vanilla_registry();
    let fox = new_fox();

    // Awake and idle: both controls run (vanilla canMove and not sleeping).
    assert!(Mob::can_move_control_tick(&fox));
    assert!(Mob::can_look_control_tick(&fox));

    // Sitting stops movement but the fox still turns its head.
    fox.set_sitting(true);
    assert!(!Mob::can_move_control_tick(&fox));
    assert!(Mob::can_look_control_tick(&fox));
    fox.set_sitting(false);

    // Sleeping stops both.
    fox.set_sleeping(true);
    assert!(!Mob::can_move_control_tick(&fox));
    assert!(!Mob::can_look_control_tick(&fox));
}

#[test]
fn fox_faceplant_goal_runs_while_faceplanted_and_stands_up_on_stop() {
    let (_world, fox) = world_with_fox("fox_faceplant");
    let mut goal = FaceplantGoal::new();

    // Not faceplanted: the goal does not run.
    assert!(!goal.can_use(fox.as_ref()));

    // Faceplanted: the goal runs, and stopping stands the fox back up.
    fox.set_faceplanted(true);
    assert!(goal.can_use(fox.as_ref()));
    goal.start(fox.as_ref());
    assert!(goal.can_continue_to_use(fox.as_ref()));
    goal.stop(fox.as_ref());
    assert!(!fox.is_faceplanted());
}
