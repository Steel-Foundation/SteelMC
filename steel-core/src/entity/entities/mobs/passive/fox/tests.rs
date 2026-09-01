use std::io::Cursor;

use simdnbt::borrow::read_compound as read_borrowed_compound;
use steel_registry::{
    REGISTRY, init_vanilla_registry, vanilla_attributes, vanilla_blocks, vanilla_entities,
    vanilla_items,
};
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use crate::behavior::init_behaviors;
use crate::entity::ai::goal::{FloatGoal, Goal};
use crate::entity::entities::PigEntity;
use crate::entity::entities::mobs::passive::fox::goals::FOX_FLOAT_WATER_DEPTH;
use crate::entity::{EntityFluidContact, SharedEntity};
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

    assert!(Mob::can_hold_item(&fox, &stone));

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

    assert!(!fox.is_alertable(), "a fox alone is not alertable");

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

    assert_eq!(offspring.variant(), FoxVariant::Snow);
}

#[test]
fn fox_pounce_goal_commits_once_it_starts() {
    // A fox in mid-air cannot be bumped off its pounce by a higher-priority goal.
    let goal = FoxPounceGoal;
    assert!(!Goal::is_interruptable(&goal));
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

    LivingEntity::drop_custom_death_equipment(fox.as_ref(), &world);

    let mut mouth_empty = false;
    fox.with_equipment_slot(EquipmentSlot::MainHand, &mut |held| {
        mouth_empty = held.is_empty();
    });
    assert!(mouth_empty, "the fox drops the held mouth item on death");

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

fn fox_holding(name: &'static str, item: ItemStack) -> (Arc<World>, Arc<FoxEntity>) {
    let (world, fox) = world_with_fox(name);
    fox.set_on_ground(true);
    fox.living_base()
        .equipment()
        .lock()
        .set(EquipmentSlot::MainHand, item);
    (world, fox)
}

fn mouth_item(fox: &FoxEntity) -> ItemStack {
    let mut held = ItemStack::empty();
    fox.with_equipment_slot(EquipmentSlot::MainHand, &mut |item_stack| {
        held = item_stack.clone();
    });
    held
}

#[test]
fn fox_swallows_the_food_in_its_mouth_once_the_timer_runs_out() {
    let (_world, fox) = fox_holding("fox_eat", ItemStack::new(&vanilla_items::SWEET_BERRIES));
    *fox.ticks_since_eaten.lock() = FOX_EAT_TICKS - 1;

    fox.tick_eating();
    assert!(
        mouth_item(&fox).is(&vanilla_items::SWEET_BERRIES),
        "the fox holds its food until the timer passes the threshold"
    );

    fox.tick_eating();
    assert!(mouth_item(&fox).is_empty(), "the fox swallows the berries");
    assert_eq!(
        *fox.ticks_since_eaten.lock(),
        0,
        "swallowing restarts the timer"
    );
}

#[test]
fn fox_is_left_holding_the_empty_bottle() {
    let (_world, fox) = fox_holding(
        "fox_eat_remainder",
        ItemStack::new(&vanilla_items::HONEY_BOTTLE),
    );
    *fox.ticks_since_eaten.lock() = FOX_EAT_TICKS;

    fox.tick_eating();

    assert!(
        mouth_item(&fox).is(&vanilla_items::GLASS_BOTTLE),
        "drinking leaves the bottle in the fox's mouth"
    );
}

#[test]
fn fox_holds_an_item_that_is_not_food_forever() {
    let (_world, fox) = fox_holding("fox_eat_non_food", ItemStack::new(&vanilla_items::EMERALD));
    *fox.ticks_since_eaten.lock() = FOX_EAT_TICKS;

    fox.tick_eating();

    assert!(
        mouth_item(&fox).is(&vanilla_items::EMERALD),
        "a fox never eats something that is not food"
    );
    assert!(
        *fox.ticks_since_eaten.lock() > FOX_EAT_TICKS,
        "the timer keeps running even when the fox cannot eat"
    );
}

#[test]
fn fox_does_not_eat_while_asleep_in_the_air_or_chasing_something() {
    let (world, fox) = fox_holding(
        "fox_eat_gated",
        ItemStack::new(&vanilla_items::SWEET_BERRIES),
    );

    let assert_still_holding_berries = |reason: &str| {
        *fox.ticks_since_eaten.lock() = FOX_EAT_TICKS;
        fox.tick_eating();
        assert!(
            mouth_item(&fox).is(&vanilla_items::SWEET_BERRIES),
            "{reason}"
        );
    };

    fox.set_sleeping(true);
    assert_still_holding_berries("a sleeping fox does not eat");
    fox.set_sleeping(false);

    fox.set_on_ground(false);
    assert_still_holding_berries("a fox in mid-air does not eat");
    fox.set_on_ground(true);

    let pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        DVec3::new(9.0, 65.0, 8.0),
        Arc::downgrade(&world),
    ));
    world
        .try_add_entity(Arc::clone(&pig) as SharedEntity)
        .expect("pig should attach to the loaded chunk");
    assert!(Mob::set_target(fox.as_ref(), Some(&(pig as SharedEntity))));
    assert_still_holding_berries("a fox chasing something does not eat");
}

struct SpawnRuleLevel {
    below_state: BlockStateId,
    raw_brightness: u8,
}

impl LevelReader for SpawnRuleLevel {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        if pos == SPAWN_POS.below() {
            return self.below_state;
        }

        REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
    }

    fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
        self.raw_brightness
    }

    fn min_y(&self) -> i32 {
        -64
    }

    fn height(&self) -> i32 {
        384
    }
}

const SPAWN_POS: BlockPos = BlockPos::new(0, 64, 0);

fn fox_spawns_on(below_state: BlockStateId, raw_brightness: u8) -> bool {
    let level = SpawnRuleLevel {
        below_state,
        raw_brightness,
    };
    <FoxEntity as Animal>::check_animal_spawn_rules(&level, EntitySpawnReason::Natural, SPAWN_POS)
}

#[test]
fn foxes_only_spawn_on_their_own_ground() {
    init_vanilla_registry();

    assert!(fox_spawns_on(vanilla_blocks::PODZOL.default_state(), 9));
    assert!(fox_spawns_on(vanilla_blocks::SNOW_BLOCK.default_state(), 9));

    assert!(!fox_spawns_on(vanilla_blocks::SAND.default_state(), 9));

    assert!(!fox_spawns_on(vanilla_blocks::PODZOL.default_state(), 8));
    let dark = SpawnRuleLevel {
        below_state: vanilla_blocks::PODZOL.default_state(),
        raw_brightness: 8,
    };
    assert!(!<FoxEntity as Animal>::check_animal_spawn_rules(
        &dark,
        EntitySpawnReason::TrialSpawner,
        SPAWN_POS
    ));
}

#[test]
fn a_fox_does_not_sleep_through_water_prey_or_a_storm() {
    let (world, fox) = world_with_fox("fox_wake");
    assert!(world.set_block(
        fox.block_position(),
        vanilla_blocks::SAND.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));

    fox.set_sleeping(true);
    fox.set_sitting(true);
    fox.set_faceplanted(true);
    fox.tick_fox_posture();
    assert!(fox.is_sleeping(), "a quiet night does not wake the fox");
    assert!(
        !fox.is_sitting(),
        "a fox that has dozed off is not sitting up as well"
    );

    let prey = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        DVec3::new(9.0, 65.0, 8.0),
        Arc::downgrade(&world),
    ));
    world
        .try_add_entity(Arc::clone(&prey) as SharedEntity)
        .expect("prey should attach to the loaded chunk");
    assert!(Mob::set_target(fox.as_ref(), Some(&(prey as SharedEntity))));
    fox.set_sleeping(true);
    fox.tick_fox_posture();
    assert!(!fox.is_sleeping(), "prey nearby wakes the fox");
}

#[test]
fn clearing_a_foxs_states_drops_everything_it_was_in_the_middle_of() {
    init_vanilla_registry();
    let fox = new_fox();
    fox.set_interested(true);
    fox.set_crouching(true);
    fox.set_sitting(true);
    fox.set_sleeping(true);
    fox.set_defending(true);
    fox.set_faceplanted(true);

    fox.clear_states();

    assert!(!fox.is_interested());
    assert!(!fox.is_crouching());
    assert!(!fox.is_sitting());
    assert!(!fox.is_sleeping());
    assert!(!fox.is_defending());
    assert!(!fox.is_faceplanted());
}

#[test]
fn a_fox_starts_swimming_in_shallower_water_than_most_mobs() {
    let (_world, fox) = world_with_fox("fox_float_depth");
    let depth = f64::midpoint(FOX_FLOAT_WATER_DEPTH, fox.get_fluid_jump_threshold());
    fox.base()
        .set_fluid_contact(EntityFluidContact::from_parts(depth, 0.0, false, false));

    let mut shared = FloatGoal::new(fox.mob_base());
    assert!(
        !shared.can_use(fox.as_ref()),
        "this is too shallow for the shared float goal, which is the point"
    );

    let mut goal = FoxFloatGoal::new(fox.mob_base());
    assert!(goal.can_use(fox.as_ref()), "a fox swims in it anyway");

    fox.set_sleeping(true);
    fox.set_sitting(true);
    goal.start(fox.as_ref());
    assert!(!fox.is_sleeping());
    assert!(!fox.is_sitting());
}

#[test]
fn a_defending_fox_neither_panics_nor_follows_its_parent() {
    let (_world, fox) = world_with_fox("fox_defending_gates");
    fox.set_defending(true);

    assert!(!FoxPanicGoal::new(2.2).can_use(fox.as_ref()));
    assert!(!FoxFollowParentGoal::new(1.25).can_use(fox.as_ref()));
    assert!(!FoxFollowParentGoal::new(1.25).can_continue_to_use(fox.as_ref()));

    fox.set_defending(false);
    fox.set_sleeping(true);
    FoxFollowParentGoal::new(1.25).start(fox.as_ref());
    assert!(!fox.is_sleeping());
}

#[test]
fn a_fox_fixed_on_something_does_not_turn_to_watch_a_player() {
    let (_world, fox) = world_with_fox("fox_look_gates");

    fox.set_interested(true);
    assert!(!FoxLookAtPlayerGoal::new(24.0).can_use(fox.as_ref()));
    assert!(!FoxLookAtPlayerGoal::new(24.0).can_continue_to_use(fox.as_ref()));

    fox.set_interested(false);
    fox.set_faceplanted(true);
    assert!(!FoxLookAtPlayerGoal::new(24.0).can_use(fox.as_ref()));
}
