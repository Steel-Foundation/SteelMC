use super::*;
use crate::behavior::blocks::PowderSnowBlock;
use crate::behavior::{ITEM_BEHAVIORS, InteractionResult, UseItemContext};
use crate::entity::next_entity_id;
use crate::inventory::container::Container as _;
use crate::inventory::equipment::EntityEquipment;
use crate::player::player_inventory::PlayerInventory;
use crate::test_support::TestPlayerBuilder;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use steel_protocol::packets::game::{ClickType, HashedStack, SContainerClick};
use steel_utils::locks::Shared;

#[test]
fn can_glide_using_matches_vanilla_component_gate() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let mut elytra = ItemStack::new(&vanilla_items::ELYTRA);

    assert!(entity.can_glide_using(&elytra, EquipmentSlot::Chest));
    assert!(!entity.can_glide_using(&elytra, EquipmentSlot::Head));

    elytra.set_damage_value(elytra.get_max_damage() - 1);

    assert!(elytra.next_damage_will_break());
    assert!(!entity.can_glide_using(&elytra, EquipmentSlot::Chest));
    assert!(!entity.can_glide_using(&ItemStack::new(&vanilla_items::STONE), EquipmentSlot::Chest));
}

#[test]
fn living_armor_cover_counts_non_empty_humanoid_armor_slots() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert_f32_close(entity.get_armor_cover_percentage(), 0.0);

    entity.equip(EquipmentSlot::Head, ItemStack::new(&vanilla_items::STONE));
    entity.equip(EquipmentSlot::Feet, ItemStack::new(&vanilla_items::STONE));

    assert_f32_close(entity.get_armor_cover_percentage(), 0.5);
}

#[test]
fn living_visibility_percent_uses_discrete_and_invisible_scaling() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert_f64_close(entity.get_visibility_percent(None), 1.0);

    EntitySyncedData::set_base_invisible_flag(&entity.entity_data, true);

    let invisible_without_armor = 0.7 * f64::from(0.1_f32);
    assert_f64_close(entity.get_visibility_percent(None), invisible_without_armor);

    entity.set_shared_shift_key_down(true);

    assert_f64_close(
        entity.get_visibility_percent(None),
        0.8 * invisible_without_armor,
    );
}

#[test]
fn living_visibility_percent_uses_matching_mob_head_disguise() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let skeleton =
        LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::SKELETON);

    entity.equip(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::SKELETON_SKULL),
    );

    assert_f64_close(entity.get_visibility_percent(Some(&skeleton)), 0.5);
}

#[test]
fn living_freeze_immunity_uses_armor_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(entity.default_living_can_freeze());

    entity.equip(
        EquipmentSlot::Feet,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(!entity.default_living_can_freeze());
}

#[test]
fn living_freeze_immunity_uses_body_armor_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity.equip(
        EquipmentSlot::Body,
        ItemStack::new(&vanilla_items::LEATHER_HORSE_ARMOR),
    );

    assert!(!entity.default_living_can_freeze());
}

#[test]
fn living_freeze_immunity_ignores_non_armor_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(entity.default_living_can_freeze());
}

#[test]
fn living_freezing_decays_when_not_in_powder_snow() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_ticks_frozen(10);

    entity.tick_freezing();

    assert_eq!(entity.ticks_frozen(), 8);
}

#[test]
fn living_freezing_keeps_ticks_while_in_powder_snow() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_ticks_frozen(10);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);

    entity.tick_freezing();

    assert_eq!(entity.ticks_frozen(), 11);
}

#[test]
fn living_freezing_adds_powder_snow_speed_modifier() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_non_air_frost_block();
    entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE / 2);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
    let base_speed = entity
        .attributes()
        .lock()
        .required_value(vanilla_attributes::MOVEMENT_SPEED);

    entity.tick_freezing();

    let attributes = entity.attributes().lock();
    assert!(attributes.has_modifier(
        vanilla_attributes::MOVEMENT_SPEED,
        &SPEED_MODIFIER_POWDER_SNOW_ID,
    ));
    assert_f64_close(
        attributes.required_value(vanilla_attributes::MOVEMENT_SPEED),
        base_speed - f64::from(0.05_f32 * entity.percent_frozen()),
    );
}

#[test]
fn living_freezing_removes_stale_powder_snow_speed_modifier() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.attributes().lock().add_modifier(
        vanilla_attributes::MOVEMENT_SPEED,
        AttributeModifier {
            id: SPEED_MODIFIER_POWDER_SNOW_ID,
            amount: -0.05,
            operation: AttributeModifierOperation::AddValue,
        },
        false,
    );

    entity.tick_freezing();

    assert!(!entity.attributes().lock().has_modifier(
        vanilla_attributes::MOVEMENT_SPEED,
        &SPEED_MODIFIER_POWDER_SNOW_ID,
    ));
}

#[test]
fn living_freezing_damages_fully_frozen_entities_on_frequency() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
    entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
    for _ in 0..40 {
        entity.advance_tick_count();
    }

    entity.tick_freezing();

    assert_f32_close(entity.get_health(), 19.0);
}

#[test]
fn default_ai_step_ticks_freezing_after_travel() {
    init_vanilla_registry();
    init_behaviors();
    let entity = Arc::new(LivingFluidTestEntity::new_in_world(
        0.0,
        0.0,
        true,
        test_world(),
    ));
    let shared_entity: SharedEntity = Arc::<LivingFluidTestEntity>::clone(&entity);
    entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
    for _ in 0..40 {
        entity.advance_tick_count();
    }

    entity.default_ai_step(&shared_entity);

    assert_eq!(
        entity.damage_type_keys(),
        vec![vanilla_damage_types::FREEZE.key.clone()]
    );
    assert_f32_close(entity.get_health(), 19.0);
}

#[test]
fn entity_cramming_damage_threshold_matches_vanilla_push_entities() {
    assert!(!should_apply_entity_cramming_damage(0, 100, 100, 0));
    assert!(!should_apply_entity_cramming_damage(24, 23, 23, 0));
    assert!(!should_apply_entity_cramming_damage(24, 24, 23, 0));
    assert!(!should_apply_entity_cramming_damage(24, 24, 24, 1));
    assert!(should_apply_entity_cramming_damage(24, 24, 24, 0));
}

#[test]
fn freezing_damage_hurts_extra_tagged_entity_types() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
        .with_entity_type(&vanilla_entities::BLAZE);

    assert!(entity.hurt(
        test_world(),
        &DamageSource::environment(&vanilla_damage_types::FREEZE),
        1.0,
    ));

    assert_f32_close(entity.get_health(), 15.0);
}

#[test]
fn living_powder_snow_walkability_uses_feet_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(!PowderSnowBlock::can_entity_walk_on_powder_snow(&entity));

    entity.equip(
        EquipmentSlot::Feet,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(PowderSnowBlock::can_entity_walk_on_powder_snow(&entity));
}

#[test]
fn living_powder_snow_walkability_ignores_non_feet_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(!PowderSnowBlock::can_entity_walk_on_powder_snow(&entity));
}

#[test]
fn default_can_glide_uses_living_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(false);

    assert!(!entity.can_glide());

    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));

    assert!(entity.can_glide());
}

#[test]
fn try_to_start_fall_flying_uses_vanilla_glider_gate() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);

    assert!(entity.try_to_start_fall_flying());
    assert!(entity.is_fall_flying());
}

#[test]
fn gliding_non_player_living_entity_forces_velocity_sync() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let entity_ref: &dyn Entity = &entity;

    assert!(!entity_ref.forces_fall_flying_velocity_sync());

    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);
    assert!(entity.try_to_start_fall_flying());

    assert!(
        entity_ref.forces_fall_flying_velocity_sync(),
        "a gliding mob should keep its velocity synced, the same as a gliding player"
    );
}

#[test]
fn try_to_start_fall_flying_rejects_levitation() {
    init_vanilla_registry();
    init_behaviors();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);
    entity.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);

    assert!(!entity.try_to_start_fall_flying());
    assert!(!entity.is_fall_flying());
}

#[test]
fn update_fall_flying_damages_glider_every_second_event_interval() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);
    for _ in 0..19 {
        entity.living_base.tick_fall_flying_state(true);
    }

    entity.update_fall_flying();

    assert_eq!(
        entity
            .living_base
            .equipment()
            .lock()
            .get_ref(EquipmentSlot::Chest)
            .get_damage_value(),
        1
    );
}

#[test]
fn update_fall_flying_stops_when_glider_gate_fails() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_fall_flying(true);

    entity.update_fall_flying();

    assert!(!entity.is_fall_flying());
}

fn equip_game_events(
    name: &'static str,
    entity: LivingFluidTestEntity,
    changes: &[(EquipmentSlot, ItemStack)],
) -> Vec<GameEventRef> {
    init_vanilla_registry();
    init_behaviors();
    let test_world = fresh_test_world(name);
    let world = &test_world.world;
    let position = DVec3::new(0.5, 64.0, 0.5);
    let section = SectionPos::from_block_pos(BlockPos::from(position));
    insert_ready_full_chunk(world, ChunkPos::new(section.x(), section.z()));
    let listener = Arc::new(RecordingGameEventListener::new(position));
    let _registration = RegisteredGameEventListener::new(
        world,
        section,
        Arc::<RecordingGameEventListener>::clone(&listener),
    );

    entity.base().set_world(Arc::downgrade(world));
    entity.base().set_position_local(position);
    for (slot, stack) in changes {
        entity.set_item_slot(*slot, stack.clone());
    }

    let events = listener.events.lock();
    events.iter().map(|(event, _)| *event).collect()
}

#[test]
fn set_item_slot_emits_equip_for_equippables_and_unequip_otherwise() {
    let events = equip_game_events(
        "set_item_slot_equip_events",
        equipped_entity(),
        &[
            (
                EquipmentSlot::Head,
                ItemStack::new(&vanilla_items::IRON_HELMET),
            ),
            (
                EquipmentSlot::MainHand,
                ItemStack::new(&vanilla_items::SWEET_BERRIES),
            ),
            (EquipmentSlot::MainHand, ItemStack::empty()),
        ],
    );

    assert_eq!(
        events,
        vec![
            &vanilla_game_events::EQUIP,
            &vanilla_game_events::UNEQUIP,
            &vanilla_game_events::UNEQUIP,
        ]
    );
}

#[test]
fn set_item_slot_stays_quiet_for_the_same_item_and_on_the_first_tick() {
    let helmet = ItemStack::new(&vanilla_items::IRON_HELMET);
    let same_item = equip_game_events(
        "set_item_slot_same_item",
        equipped_entity(),
        &[
            (EquipmentSlot::Head, helmet.clone()),
            (EquipmentSlot::Head, helmet.clone()),
        ],
    );
    assert_eq!(same_item, vec![&vanilla_game_events::EQUIP]);

    let on_first_tick = equipped_entity();
    on_first_tick.base().set_first_tick(true);
    let first_tick = equip_game_events(
        "set_item_slot_first_tick",
        on_first_tick,
        &[(EquipmentSlot::Head, helmet)],
    );
    assert_eq!(first_tick, Vec::<GameEventRef>::new());
}

fn equipped_entity() -> LivingFluidTestEntity {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.base().set_first_tick(false);
    entity
}

#[test]
fn set_item_slot_treats_an_equippable_in_the_wrong_slot_as_an_equip() {
    let helmet = ItemStack::new(&vanilla_items::IRON_HELMET);
    let events = equip_game_events(
        "set_item_slot_wrong_slot",
        equipped_entity(),
        &[(EquipmentSlot::MainHand, helmet.clone())],
    );

    assert_eq!(events, vec![&vanilla_game_events::EQUIP]);
    assert!(
        LivingEntity::equip_sound(&equipped_entity(), EquipmentSlot::MainHand, &helmet).is_none(),
        "a helmet held in the hand is not worn, so it makes no equip sound"
    );
}

#[test]
fn set_item_slot_stays_quiet_for_a_spectator() {
    let events = equip_game_events(
        "set_item_slot_spectator",
        equipped_entity().with_spectator(),
        &[(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::IRON_HELMET),
        )],
    );

    assert_eq!(events, Vec::<GameEventRef>::new());
}

#[test]
fn equipping_armor_from_the_hand_runs_the_equip_hook() {
    init_vanilla_registry();
    init_behaviors();
    let test_world = fresh_test_world("use_item_equips_armor");
    let world = &test_world.world;
    let position = DVec3::new(0.5, 64.0, 0.5);
    let section = SectionPos::from_block_pos(BlockPos::from(position));
    insert_ready_full_chunk(world, ChunkPos::new(section.x(), section.z()));
    let listener = Arc::new(RecordingGameEventListener::new(position));
    let _registration = RegisteredGameEventListener::new(
        world,
        section,
        Arc::<RecordingGameEventListener>::clone(&listener),
    );

    let player = TestPlayerBuilder::new(Arc::clone(world), "Equipper", next_entity_id()).build();
    assert!(player.try_set_position(position).is_ok());
    player.base().set_first_tick(false);
    player.inventory.lock().set_item_in_hand(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::IRON_HELMET),
    );

    let behavior = ITEM_BEHAVIORS.get_behavior(&vanilla_items::IRON_HELMET);
    let mut context = UseItemContext::new(
        &player,
        InteractionHand::MainHand,
        world,
        Arc::clone(&player.inventory),
    );

    assert_eq!(behavior.use_item(&mut context), InteractionResult::Success);
    assert!(
        EntityEquipment::get_ref(&*player.inventory.lock(), EquipmentSlot::Head)
            .is(&vanilla_items::IRON_HELMET)
    );
    let events: Vec<GameEventRef> = listener.events.lock().iter().map(|(e, _)| *e).collect();
    assert_eq!(events, vec![&vanilla_game_events::EQUIP]);
}

/// Records, for each game event, whether the player's inventory was free and how many helmets it held.
struct InventoryProbe {
    position: DVec3,
    inventory: Shared<PlayerInventory>,
    seen: SyncMutex<Vec<Option<usize>>>,
}

impl GameEventListener for InventoryProbe {
    fn listener_pos(&self) -> Option<DVec3> {
        Some(self.position)
    }

    fn listener_radius(&self) -> i32 {
        16
    }

    fn handle_game_event(
        &self,
        _world: &Arc<World>,
        _event: GameEventRef,
        _context: &GameEventContext<'_>,
        _source_pos: DVec3,
    ) -> bool {
        let helmets = self.inventory.try_lock().map(|inventory| {
            (0..inventory.get_container_size())
                .filter(|&slot| inventory.get_item(slot).is(&vanilla_items::IRON_HELMET))
                .count()
        });
        self.seen.lock().push(helmets);
        true
    }
}

#[test]
fn shift_clicking_armor_announces_the_equip_once_the_inventory_is_settled() {
    const HOTBAR_FIRST_MENU_SLOT: i16 = 36;

    init_vanilla_registry();
    init_behaviors();
    let test_world = fresh_test_world("quick_move_armor_equip_event");
    let world = &test_world.world;
    let position = DVec3::new(0.5, 64.0, 0.5);
    let section = SectionPos::from_block_pos(BlockPos::from(position));
    insert_ready_full_chunk(world, ChunkPos::new(section.x(), section.z()));

    let player =
        TestPlayerBuilder::new(Arc::clone(world), "QuickEquipper", next_entity_id()).build();
    assert!(player.try_set_position(position).is_ok());
    player.base().set_first_tick(false);
    player
        .inventory
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::IRON_HELMET));

    let probe = Arc::new(InventoryProbe {
        position,
        inventory: Arc::clone(&player.inventory),
        seen: SyncMutex::new(Vec::new()),
    });
    let _registration =
        RegisteredGameEventListener::new(world, section, Arc::<InventoryProbe>::clone(&probe));

    player.handle_container_click(SContainerClick {
        container_id: 0,
        state_id: 0,
        slot_num: HOTBAR_FIRST_MENU_SLOT,
        button_num: 0,
        click_type: ClickType::QuickMove,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });

    assert!(
        EntityEquipment::get_ref(&*player.inventory.lock(), EquipmentSlot::Head)
            .is(&vanilla_items::IRON_HELMET)
    );
    assert_eq!(*probe.seen.lock(), vec![Some(1)]);
}
