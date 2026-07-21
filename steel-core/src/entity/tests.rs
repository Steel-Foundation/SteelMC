use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_protocol::packets::game::RelativeMovement;
use steel_registry::blocks::{
    block_state_ext::BlockStateExt as _,
    properties::{BlockStateProperties, Direction as BlockDirection},
};
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidState;
use steel_registry::game_events::GameEventRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::LivingEntityData as SyncedLivingEntityData;
use steel_registry::{
    REGISTRY, sound_events, test_support::init_test_registry, vanilla_attributes, vanilla_blocks,
    vanilla_damage_types, vanilla_entities, vanilla_fluids, vanilla_game_events, vanilla_items,
    vanilla_loot_tables, vanilla_mob_effects,
};
use steel_utils::Downcast as _;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{
    BlockPos, BlockStateId, Direction, Identifier, SectionPos, WorldAabb, axis::Axis,
    block_util::FoundRectangle,
};
use text_components::{Modifier as _, TextComponent, format::Color, interactivity::ClickEvent};
use uuid::Uuid;

use crate::behavior::init_behaviors;
use crate::chunk_saver::ChunkStorage;
use crate::entity::damage::DamageSource;
use crate::entity::entities::PigEntity;
use crate::entity::mob::Mob;
use crate::inventory::equipment::EquipmentSlot;
use crate::portal::PortalKind;
use crate::test_support::{cross_world_damage_test_world, test_world};
use crate::world::game_event_context::GameEventContext;
use crate::world::game_event_listener::{GameEventListener, SharedGameEventListener};
use crate::world::{LevelReader, World};

use super::{
    ActiveMobEffect, AttributeModifier, AttributeModifierOperation, DAMAGE_KNOCKBACK_POWER,
    DEFAULT_SWING_DURATION, DEFAULT_TICKS_REQUIRED_TO_FREEZE, Entity, EntityBase,
    EntityFluidContact, EntityLevelCallback, EntityMoveError, EntityOwnership, EntitySyncedData,
    EntityVerticalMovementStateUpdate, InsideBlockEffectType, LivingEntity, LivingEntityBase,
    LivingTravelInput, RemovalReason, SPEED_MODIFIER_POWDER_SNOW_ID, SharedEntity,
    block_state_suffocates_eye_box, closest_open_space_direction, fall_damage_reset_clip_target,
    fall_flying_collision_damage, fall_flying_free_fall_interval, get_input_vector,
    indirect_passengers, passenger_transition_position, passenger_transition_rotation,
    remove_after_changing_dimensions, should_apply_entity_cramming_damage,
    should_apply_resolved_movement, start_riding_entities, transfer_leashables_to_holder,
    trapdoor_usable_as_ladder_state,
};

struct PushableTestEntity {
    base: EntityBase,
}

impl PushableTestEntity {
    fn shared(id: i32, position: DVec3) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::new(id, position, vanilla_entities::ITEM.dimensions, Weak::new()),
        })
    }
}

crate::entity::impl_test_downcast_type!(PushableTestEntity);

impl Entity for PushableTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn is_pushable(&self) -> bool {
        true
    }
}

struct TypedTestEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    projectile_owner_uuid: Option<Uuid>,
}

impl TypedTestEntity {
    fn new(id: i32, entity_type: EntityTypeRef) -> Self {
        Self {
            base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
            entity_type,
            projectile_owner_uuid: None,
        }
    }

    fn projectile_with_owner_uuid(id: i32, owner_uuid: Uuid) -> Self {
        Self {
            base: EntityBase::new(
                id,
                DVec3::ZERO,
                vanilla_entities::ENDER_PEARL.dimensions,
                Weak::new(),
            ),
            entity_type: &vanilla_entities::ENDER_PEARL,
            projectile_owner_uuid: Some(owner_uuid),
        }
    }
}

crate::entity::impl_test_downcast_type!(TypedTestEntity);

impl Entity for TypedTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn projectile_owner_uuid(&self) -> Option<Uuid> {
        self.projectile_owner_uuid
    }
}

#[test]
fn non_player_command_identity_uses_uuid_and_resolved_name() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::PIG);

    assert_eq!(entity.scoreboard_name(), entity.uuid().to_string());
    assert_eq!(entity.plain_text_name(), "Pig");

    entity.set_custom_name(Some(TextComponent::plain("Command Pig")));
    assert_eq!(entity.plain_text_name(), "Command Pig");
}

#[test]
fn entity_display_name_preserves_the_custom_name_component() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::PIG);
    let custom_name = TextComponent::plain("Command Pig")
        .color(Color::Red)
        .click_event(ClickEvent::run_command("/root-action"))
        .add_child(
            TextComponent::plain(" Child")
                .italic(true)
                .click_event(ClickEvent::run_command("/child-action")),
        );
    entity.set_custom_name(Some(custom_name.clone()));

    let display_name = entity.display_name();
    let expected_insertion = entity.uuid().to_string();

    assert_eq!(display_name.content, custom_name.content);
    assert_eq!(display_name.format, custom_name.format);
    assert_eq!(display_name.children.len(), 1);
    assert_eq!(
        display_name.children[0].content,
        custom_name.children[0].content
    );
    assert_eq!(
        display_name.children[0].format,
        custom_name.children[0].format
    );
    assert!(display_name.interactions.click.is_none());
    assert!(
        display_name
            .children
            .iter()
            .all(|child| child.interactions.click.is_none())
    );
    assert_eq!(
        display_name.interactions.insertion.as_deref(),
        Some(expected_insertion.as_str())
    );
    assert!(display_name.interactions.hover.is_some());
}

#[test]
fn command_data_compare_nbt_contains_base_and_custom_data() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::PIG);
    entity.set_velocity(DVec3::new(0.25, -0.5, 0.75));
    entity.set_rotation((45.0, 10.0));
    entity.set_on_ground(true);
    entity.add_tag("selected".to_owned());
    let mut custom_data = NbtCompound::new();
    custom_data.insert("flag", NbtTag::Byte(1));
    entity.set_custom_data(custom_data);

    let nbt = entity.nbt_for_data_compare();

    assert_eq!(
        nbt.get("Motion"),
        Some(&NbtTag::List(NbtList::Double(vec![0.25, -0.5, 0.75])))
    );
    assert_eq!(
        nbt.get("Rotation"),
        Some(&NbtTag::List(NbtList::Float(vec![45.0, 10.0])))
    );
    assert_eq!(nbt.get("OnGround"), Some(&NbtTag::Byte(1)));
    assert_eq!(
        nbt.compound("data").and_then(|data| data.byte("flag")),
        Some(1)
    );
    assert!(matches!(
        nbt.get("Tags"),
        Some(NbtTag::List(NbtList::String(tags)))
            if tags.len() == 1 && tags[0].to_str() == "selected"
    ));
}

#[test]
fn command_data_compare_nbt_contains_implemented_living_data() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_health(12.5);
    entity
        .attributes()
        .lock()
        .set_base_value(vanilla_attributes::MAX_ABSORPTION, 3.0);
    entity.set_absorption_amount(3.0);
    entity.living_base.increment_death_time();
    entity.living_base.apply_post_impulse_grace_time(7);
    entity
        .living_base
        .set_ignore_fall_damage_from_current_impulse(true, DVec3::new(1.0, 2.0, 3.0));
    entity.set_fall_flying(true);
    entity.set_sleeping_pos(BlockPos::new(4, 5, 6));
    entity.add_mob_effect(
        ActiveMobEffect::with_duration(vanilla_mob_effects::HASTE, 200, 2)
            .with_ambient(true)
            .with_visible(false),
    );
    entity.equip(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );

    let nbt = entity.nbt_for_data_compare();

    assert_eq!(nbt.get("Health"), Some(&NbtTag::Float(12.5)));
    assert_eq!(nbt.get("DeathTime"), Some(&NbtTag::Short(1)));
    assert_eq!(nbt.get("AbsorptionAmount"), Some(&NbtTag::Float(3.0)));
    assert_eq!(
        nbt.get("current_impulse_context_reset_grace_time"),
        Some(&NbtTag::Int(40))
    );
    assert_eq!(
        nbt.get("current_explosion_impact_pos"),
        Some(&NbtTag::List(NbtList::Double(vec![1.0, 2.0, 3.0])))
    );
    assert_eq!(nbt.get("FallFlying"), Some(&NbtTag::Byte(1)));
    assert_eq!(
        nbt.get("sleeping_pos"),
        Some(&NbtTag::IntArray(vec![4, 5, 6]))
    );

    let Some(NbtTag::List(NbtList::Compound(attributes))) = nbt.get("attributes") else {
        panic!("living attributes should be serialized");
    };
    assert!(attributes.iter().any(|attribute| {
        attribute.string("id").is_some_and(|id| {
            id.to_str().as_ref() == vanilla_attributes::MAX_HEALTH.key.to_string()
        })
    }));

    let Some(NbtTag::List(NbtList::Compound(effects))) = nbt.get("active_effects") else {
        panic!("active effects should be serialized");
    };
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].string("id").map(ToString::to_string),
        Some("minecraft:haste".to_owned())
    );
    assert_eq!(effects[0].byte("amplifier"), Some(2));
    assert_eq!(effects[0].int("duration"), Some(200));
    assert_eq!(effects[0].byte("ambient"), Some(1));
    assert_eq!(effects[0].byte("show_particles"), Some(0));
    assert_eq!(effects[0].byte("show_icon"), Some(1));

    let Some(NbtTag::Compound(equipment)) = nbt.get("equipment") else {
        panic!("living equipment should be serialized");
    };
    assert_eq!(
        equipment
            .compound("head")
            .and_then(|item| item.string("id"))
            .map(ToString::to_string),
        Some("minecraft:diamond_helmet".to_owned())
    );
}

#[test]
fn kill_uses_vanilla_living_and_non_living_paths() {
    let source_world = test_world();
    let target_world = cross_world_damage_test_world();
    assert!(!Arc::ptr_eq(source_world, target_world));
    let non_living_position = DVec3::new(0.25, 64.75, -0.125);
    let living_position = DVec3::new(1.25, 64.75, -0.125);
    let listener_position = DVec3::new(0.75, 64.75, -0.125);
    let listener_section = SectionPos::from_block_pos(BlockPos::from(listener_position));
    let target_listener = Arc::new(RecordingGameEventListener::new(listener_position));
    let target_shared_listener: SharedGameEventListener = target_listener.clone();
    let _target_registration = RegisteredGameEventListener::new(
        target_world,
        listener_section,
        Arc::clone(&target_shared_listener),
    );
    let source_listener = Arc::new(RecordingGameEventListener::new(listener_position));
    let source_shared_listener: SharedGameEventListener = source_listener.clone();
    let _source_registration = RegisteredGameEventListener::new(
        source_world,
        listener_section,
        Arc::clone(&source_shared_listener),
    );

    let non_living = TypedTestEntity::new(1, &vanilla_entities::ITEM);
    non_living.base().set_world(Arc::downgrade(target_world));
    non_living.base().set_position_local(non_living_position);
    non_living.kill(source_world);
    assert_eq!(non_living.removal_reason(), Some(RemovalReason::Killed));

    let living = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, target_world);
    living.base().set_position_local(living_position);
    living.kill(source_world);
    assert!(
        living
            .damage_types
            .lock()
            .iter()
            .any(|damage_type| damage_type == &vanilla_damage_types::GENERIC_KILL.key)
    );
    assert_eq!(
        living.damage_world_keys(),
        vec![source_world.key.to_string()]
    );
    assert_f32_close(living.get_health(), 0.0);
    assert_eq!(living.pose(), EntityPose::Dying);
    let Some(last_damage_source) = living.last_damage_source() else {
        panic!("kill damage should be timestamped in the victim world");
    };
    assert_eq!(
        last_damage_source.damage_type,
        &vanilla_damage_types::GENERIC_KILL
    );

    let events = target_listener.events.lock();
    assert_eq!(events.len(), 3);
    assert_eq!(
        matching_game_event_count(
            &events,
            &vanilla_game_events::ENTITY_DIE,
            non_living_position,
        ),
        1
    );
    assert_eq!(
        matching_game_event_count(&events, &vanilla_game_events::ENTITY_DIE, living_position),
        1
    );
    assert_eq!(
        matching_game_event_count(
            &events,
            &vanilla_game_events::ENTITY_DAMAGE,
            living_position,
        ),
        1
    );
    assert!(source_listener.events.lock().is_empty());
}

fn matching_game_event_count(
    events: &[(GameEventRef, DVec3)],
    expected_event: GameEventRef,
    expected_position: DVec3,
) -> usize {
    events
        .iter()
        .filter(|(event, position)| *event == expected_event && *position == expected_position)
        .count()
}

struct RegisteredGameEventListener<'a> {
    world: &'a Arc<World>,
    section: SectionPos,
    listener: SharedGameEventListener,
}

impl<'a> RegisteredGameEventListener<'a> {
    fn new(world: &'a Arc<World>, section: SectionPos, listener: SharedGameEventListener) -> Self {
        world.register_game_event_listener(section, Arc::clone(&listener));
        Self {
            world,
            section,
            listener,
        }
    }
}

impl Drop for RegisteredGameEventListener<'_> {
    fn drop(&mut self) {
        self.world
            .unregister_game_event_listener(self.section, &self.listener);
    }
}

struct RecordingGameEventListener {
    position: DVec3,
    events: SyncMutex<Vec<(GameEventRef, DVec3)>>,
}

impl RecordingGameEventListener {
    fn new(position: DVec3) -> Self {
        Self {
            position,
            events: SyncMutex::new(Vec::new()),
        }
    }
}

impl GameEventListener for RecordingGameEventListener {
    fn listener_pos(&self) -> Option<DVec3> {
        Some(self.position)
    }

    fn listener_radius(&self) -> i32 {
        16
    }

    fn handle_game_event(
        &self,
        _world: &Arc<World>,
        event: GameEventRef,
        _context: &GameEventContext<'_>,
        source_pos: DVec3,
    ) -> bool {
        self.events.lock().push((event, source_pos));
        true
    }
}

#[test]
fn entity_downcast_uses_concrete_type_key_not_registry_type() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::ITEM);
    let entity_ref: &dyn Entity = &entity;

    assert!(entity_ref.is::<TypedTestEntity>());
    assert!(entity_ref.downcast_ref::<TypedTestEntity>().is_some());
    assert!(entity_ref.downcast_ref::<PushableTestEntity>().is_none());
}

struct LeashNotificationTestEntity {
    base: EntityBase,
    holder_notifications: SyncMutex<Vec<i32>>,
    removed_notifications: SyncMutex<Vec<i32>>,
}

impl LeashNotificationTestEntity {
    fn new(id: i32) -> Arc<Self> {
        Self::with_position(id, DVec3::ZERO)
    }

    fn with_position(id: i32, position: DVec3) -> Arc<Self> {
        Arc::new(Self {
            base: EntityBase::new(id, position, vanilla_entities::ITEM.dimensions, Weak::new()),
            holder_notifications: SyncMutex::new(Vec::new()),
            removed_notifications: SyncMutex::new(Vec::new()),
        })
    }

    fn holder_notifications(&self) -> Vec<i32> {
        self.holder_notifications.lock().clone()
    }

    fn removed_notifications(&self) -> Vec<i32> {
        self.removed_notifications.lock().clone()
    }
}

crate::entity::impl_test_downcast_type!(LeashNotificationTestEntity);

impl Entity for LeashNotificationTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn notify_leash_holder(&self, leashable: &dyn Entity) {
        self.holder_notifications.lock().push(leashable.id());
    }

    fn notify_leashee_removed(&self, leashable: &dyn Entity) {
        self.removed_notifications.lock().push(leashable.id());
    }
}

struct MultiPassengerTestEntity {
    base: EntityBase,
}

impl MultiPassengerTestEntity {
    fn shared(id: i32) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::new(
                id,
                DVec3::ZERO,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
        })
    }
}

crate::entity::impl_test_downcast_type!(MultiPassengerTestEntity);

impl Entity for MultiPassengerTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn can_add_passenger(&self, _passenger: &dyn Entity) -> bool {
        true
    }
}

struct CommitRejectingCallback {
    entity_id: i32,
}

impl EntityLevelCallback for CommitRejectingCallback {
    fn validate_move(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_move_committed(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Err(EntityMoveError::NotLive {
            entity_id: self.entity_id,
        })
    }

    fn on_remove(&self, _reason: RemovalReason) {}
}

struct KnownMovementTestEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    known_movement: DVec3,
    known_speed: DVec3,
    uses_client_movement_packets: bool,
}

impl KnownMovementTestEntity {
    fn shared(
        id: i32,
        entity_type: EntityTypeRef,
        known_movement: DVec3,
        known_speed: DVec3,
    ) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
            entity_type,
            known_movement,
            known_speed,
            uses_client_movement_packets: entity_type == &vanilla_entities::PLAYER,
        })
    }
}

crate::entity::impl_test_downcast_type!(KnownMovementTestEntity);

impl Entity for KnownMovementTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn known_movement(&self) -> DVec3 {
        self.known_movement
    }

    fn known_speed(&self) -> DVec3 {
        self.known_speed
    }

    fn uses_client_movement_packets(&self) -> bool {
        self.uses_client_movement_packets
    }
}

struct LivingFluidTestEntity {
    base: EntityBase,
    living_base: LivingEntityBase,
    entity_data: SyncMutex<SyncedLivingEntityData>,
    health: SyncMutex<f32>,
    damage_types: SyncMutex<Vec<Identifier>>,
    damage_world_keys: SyncMutex<Vec<String>>,
    entity_type: EntityTypeRef,
    affected_by_fluids: bool,
    can_stand_on_fluid: bool,
    vehicle: bool,
    on_non_air_block_for_frost: bool,
    in_wall_for_base_tick: bool,
    flying_player: bool,
}

impl LivingFluidTestEntity {
    fn new(water_height: f64, lava_height: f64, affected_by_fluids: bool) -> Self {
        let base = EntityBase::new(
            1,
            DVec3::ZERO,
            vanilla_entities::PLAYER.dimensions,
            Weak::new(),
        );
        base.set_fluid_contact(EntityFluidContact::from_parts(
            water_height,
            lava_height,
            false,
            false,
        ));
        Self {
            base,
            living_base: LivingEntityBase::new(&vanilla_entities::PLAYER),
            entity_data: SyncMutex::new(SyncedLivingEntityData::new()),
            health: SyncMutex::new(20.0),
            damage_types: SyncMutex::new(Vec::new()),
            damage_world_keys: SyncMutex::new(Vec::new()),
            entity_type: &vanilla_entities::PLAYER,
            affected_by_fluids,
            can_stand_on_fluid: false,
            vehicle: false,
            on_non_air_block_for_frost: false,
            in_wall_for_base_tick: false,
            flying_player: false,
        }
    }

    fn new_in_world(
        water_height: f64,
        lava_height: f64,
        affected_by_fluids: bool,
        world: &Arc<World>,
    ) -> Self {
        let entity = Self::new(water_height, lava_height, affected_by_fluids);
        entity.base.set_world(Arc::downgrade(world));
        entity
    }

    const fn with_standing_on_fluid(mut self) -> Self {
        self.can_stand_on_fluid = true;
        self
    }

    const fn with_entity_type(mut self, entity_type: EntityTypeRef) -> Self {
        self.entity_type = entity_type;
        self
    }

    const fn with_vehicle(mut self) -> Self {
        self.vehicle = true;
        self
    }

    const fn with_non_air_frost_block(mut self) -> Self {
        self.on_non_air_block_for_frost = true;
        self
    }

    const fn with_in_wall_for_base_tick(mut self) -> Self {
        self.in_wall_for_base_tick = true;
        self
    }

    const fn with_flying_player(mut self) -> Self {
        self.flying_player = true;
        self
    }

    fn with_health(self, health: f32) -> Self {
        *self.health.lock() = health;
        self
    }

    fn damage_type_keys(&self) -> Vec<Identifier> {
        self.damage_types.lock().clone()
    }

    fn damage_world_keys(&self) -> Vec<String> {
        self.damage_world_keys.lock().clone()
    }

    fn with_eye_in_water(self) -> Self {
        let contact = self.base.fluid_contact();
        self.base.set_fluid_contact(EntityFluidContact::from_parts(
            contact.water_height(),
            contact.lava_height(),
            true,
            contact.eye_in_lava(),
        ));
        self
    }

    fn equip(&self, slot: EquipmentSlot, stack: ItemStack) {
        self.living_base.equipment().lock().set(slot, stack);
    }
}

crate::entity::impl_test_downcast_type!(LivingFluidTestEntity);

impl Entity for LivingFluidTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn is_vehicle(&self) -> bool {
        self.vehicle
    }

    fn get_default_gravity(&self) -> f64 {
        LivingEntity::get_attribute_gravity(self)
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.damage_types
            .lock()
            .push(source.damage_type.key.clone());
        self.damage_world_keys.lock().push(world.key.to_string());
        LivingEntity::hurt_server(self, world, source, amount)
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn is_flying_player(&self) -> bool {
        self.flying_player
    }
}

impl LivingEntity for LivingFluidTestEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.health.lock()
    }

    fn set_health(&self, health: f32) {
        *self.health.lock() = health.clamp(0.0, self.get_max_health());
    }

    fn is_affected_by_fluids(&self) -> bool {
        self.affected_by_fluids
    }

    fn can_stand_on_fluid(&self, _fluid_state: FluidState) -> bool {
        self.can_stand_on_fluid
    }

    fn is_on_non_air_block_for_frost(&self) -> bool {
        self.on_non_air_block_for_frost
    }

    fn is_in_wall(&self) -> bool {
        !self.is_sleeping() && (self.in_wall_for_base_tick || Entity::is_in_wall(self))
    }
}

struct ControlledVehicleTestEntity {
    base: EntityBase,
    controller: Option<SharedEntity>,
}

struct EmptyTestLevel;

impl LevelReader for EmptyTestLevel {
    fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
        REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
    }

    fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
        15
    }

    fn min_y(&self) -> i32 {
        -64
    }

    fn height(&self) -> i32 {
        384
    }
}

impl ControlledVehicleTestEntity {
    fn shared(id: i32, controller: Option<SharedEntity>) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::new(
                id,
                DVec3::ZERO,
                vanilla_entities::ACACIA_BOAT.dimensions,
                Weak::new(),
            ),
            controller,
        })
    }
}

crate::entity::impl_test_downcast_type!(ControlledVehicleTestEntity);

impl Entity for ControlledVehicleTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ACACIA_BOAT
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        self.controller.clone()
    }
}

fn assert_vec3_close(left: DVec3, right: DVec3) {
    let diff = left - right;
    assert!(
        diff.length_squared() < 1.0e-12,
        "expected {left:?} to equal {right:?}"
    );
}

fn assert_f32_close(left: f32, right: f32) {
    assert!(
        (left - right).abs() <= f32::EPSILON,
        "expected {left} to equal {right}"
    );
}

fn assert_f64_close(left: f64, right: f64) {
    assert!(
        (left - right).abs() <= 1.0e-12,
        "expected {left} to equal {right}"
    );
}

#[test]
fn living_relative_portal_position_resets_forward_offset() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity
        .base()
        .set_position_local(DVec3::new(12.0, 66.0, 20.75));
    let portal_area = FoundRectangle {
        min_corner: BlockPos::new(10, 64, 20),
        axis1_size: 4,
        axis2_size: 5,
    };
    let dimensions = entity.dimensions_for_pose(entity.pose());

    assert_vec3_close(
        entity.get_relative_portal_position(Axis::X, portal_area),
        DVec3::new(
            0.5,
            2.0 / (f64::from(portal_area.axis2_size) - f64::from(dimensions.height)),
            0.0,
        ),
    );
}

fn closest_direction_with_blocked_neighbors(
    fractional_position: DVec3,
    blocked_directions: &[Direction],
) -> Direction {
    let origin = BlockPos::ZERO;
    closest_open_space_direction(origin, fractional_position, |neighbor_pos| {
        blocked_directions
            .iter()
            .any(|direction| direction.relative(origin) == neighbor_pos)
    })
}

#[test]
fn default_tick_runs_vanilla_entity_base_tick() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);
    entity.base().set_boarding_cooldown(2);
    entity.base().set_portal_cooldown(2);

    entity.default_tick();

    assert_eq!(entity.base().boarding_cooldown(), 1);
    assert_eq!(entity.base().portal_cooldown(), 1);
}

#[test]
fn can_use_portal_requires_alive_entity() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);
    assert!(entity.can_use_portal(false));

    entity.set_removed(RemovalReason::Discarded);

    assert!(!entity.can_use_portal(true));
}

#[test]
fn static_vanilla_portal_overrides_reject_special_entities() {
    let fishing_hook = TypedTestEntity::new(1, &vanilla_entities::FISHING_BOBBER);
    let dragon = TypedTestEntity::new(2, &vanilla_entities::ENDER_DRAGON);
    let wither = TypedTestEntity::new(3, &vanilla_entities::WITHER);

    assert!(!fishing_hook.can_use_portal(true));
    assert!(!dragon.can_use_portal(true));
    assert!(!wither.can_use_portal(true));
}

#[test]
fn projectile_owner_uuid_reports_projectile_owner_identity() {
    let owner_uuid = Uuid::from_u128(42);
    let pearl = TypedTestEntity::projectile_with_owner_uuid(1, owner_uuid);
    let no_player_owner = TypedTestEntity::new(3, &vanilla_entities::ENDER_PEARL);

    assert_eq!(pearl.projectile_owner_uuid(), Some(owner_uuid));
    assert_eq!(no_player_owner.projectile_owner_uuid(), None);
}

#[test]
fn can_use_portal_respects_passenger_gate() {
    init_test_registry();

    let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
    let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);
    assert!(start_riding_entities(&passenger, &vehicle));

    assert!(!passenger.can_use_portal(false));
    assert!(passenger.can_use_portal(true));
}

#[test]
fn indirect_passengers_match_vanilla_preorder() {
    let vehicle = MultiPassengerTestEntity::shared(1);
    let first = MultiPassengerTestEntity::shared(2);
    let second = MultiPassengerTestEntity::shared(3);
    let nested = MultiPassengerTestEntity::shared(4);

    EntityBase::restore_passenger_relationship(&vehicle, &first);
    EntityBase::restore_passenger_relationship(&vehicle, &second);
    EntityBase::restore_passenger_relationship(&first, &nested);

    let passenger_ids = indirect_passengers(vehicle.as_ref())
        .into_iter()
        .map(|passenger| passenger.id())
        .collect::<Vec<_>>();

    assert_eq!(passenger_ids, vec![2, 4, 3]);
}

#[test]
fn passenger_transition_rotation_matches_vanilla_relative_flags() {
    let vehicle_rotation = (30.0, 10.0);
    let passenger_rotation = (70.0, -5.0);

    assert_eq!(
        passenger_transition_rotation(
            (90.0, 20.0),
            RelativeMovement::NONE,
            vehicle_rotation,
            passenger_rotation,
        ),
        (130.0, 5.0),
    );
    assert_eq!(
        passenger_transition_rotation(
            (15.0, -3.0),
            RelativeMovement::ROTATION,
            vehicle_rotation,
            passenger_rotation,
        ),
        (15.0, -3.0),
    );
    assert_eq!(
        passenger_transition_rotation(
            (-90.0, 0.0),
            RelativeMovement::new(RelativeMovement::X_ROT),
            vehicle_rotation,
            passenger_rotation,
        ),
        (-50.0, 0.0),
    );
}

#[test]
fn passenger_transition_position_preserves_vehicle_offset() {
    assert_eq!(
        passenger_transition_position(
            DVec3::new(100.0, 70.0, -40.0),
            RelativeMovement::NONE,
            DVec3::new(10.0, 64.0, 20.0),
            DVec3::new(12.5, 65.0, 17.0),
        ),
        DVec3::new(102.5, 71.0, -43.0),
    );
}

#[test]
fn dimension_transition_persistence_keeps_non_chunk_serializable_entities() {
    let entity: SharedEntity = Arc::new(TypedTestEntity::new(1, &vanilla_entities::FISHING_BOBBER));
    entity
        .base()
        .set_position_local(DVec3::new(12.25, 64.0, -8.75));
    entity.set_rotation((45.0, -10.0));
    entity.set_velocity(DVec3::new(0.1, 0.2, 0.3));

    assert!(ChunkStorage::entity_tree_to_persistent(&entity).is_none());
    let persistent = ChunkStorage::entity_to_dimension_transition_persistent(&entity)
        .expect("dimension transitions mirror vanilla saveWithoutId without chunk-save filtering");

    assert_eq!(persistent.entity_type, vanilla_entities::FISHING_BOBBER.key);
    assert_eq!(
        persistent.pos.map(f64::to_bits),
        [12.25_f64, 64.0, -8.75].map(f64::to_bits),
    );
    assert_eq!(
        persistent.rotation.map(f32::to_bits),
        [45.0_f32, -10.0].map(f32::to_bits),
    );
    assert_eq!(
        persistent.motion.map(f64::to_bits),
        [0.1_f64, 0.2, 0.3].map(f64::to_bits),
    );
}

#[test]
fn remove_after_changing_dimensions_clears_old_mob_leash_and_equipment() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::new(1.0, 0.0, 0.0),
        Weak::new(),
    ));
    let Some(mob) = pig.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&holder));
    pig.living_base().equipment().lock().set(
        EquipmentSlot::Saddle,
        ItemStack::new(&vanilla_items::SADDLE),
    );

    remove_after_changing_dimensions(&pig);

    assert!(!mob.is_leashed());
    assert!(
        pig.living_base()
            .equipment()
            .lock()
            .get_ref(EquipmentSlot::Saddle)
            .is_empty()
    );
}

#[test]
fn can_use_portal_rejects_sleeping_living_entities() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    assert!(entity.can_use_portal(false));

    entity.set_sleeping_pos(BlockPos::ZERO);

    assert!(!entity.can_use_portal(false));
}

#[test]
fn dimension_changing_delay_uses_vanilla_class_overrides() {
    let base = TypedTestEntity::new(1, &vanilla_entities::ITEM);
    assert_eq!(base.dimension_changing_delay(), 300);

    let minecart = TypedTestEntity::new(2, &vanilla_entities::MINECART);
    assert_eq!(minecart.dimension_changing_delay(), 10);

    let arrow = TypedTestEntity::new(3, &vanilla_entities::ARROW);
    assert_eq!(arrow.dimension_changing_delay(), 2);
}

#[test]
fn set_as_inside_portal_starts_portal_process_when_not_on_cooldown() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::ITEM);
    let entry_position = BlockPos::new(2, 64, 2);

    entity.set_as_inside_portal(PortalKind::Nether, entry_position);

    let process = entity.base().portal_process().expect("portal process");
    assert_eq!(process.portal(), PortalKind::Nether);
    assert_eq!(process.entry_position(), entry_position);
}

#[test]
fn set_as_inside_portal_resets_cooldown_without_starting_process() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::ARROW);
    entity.set_portal_cooldown(1);

    entity.set_as_inside_portal(PortalKind::Nether, BlockPos::new(2, 64, 2));

    assert_eq!(entity.portal_cooldown(), 2);
    assert_eq!(entity.base().portal_process(), None);
}

#[test]
fn living_tick_state_decrements_last_hurt_by_player_memory() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let player_uuid = Uuid::from_u128(42);
    entity.set_last_hurt_by_player(player_uuid, 1);

    entity.tick_living_state();

    assert_eq!(
        entity.living_base().last_hurt_by_player_uuid(),
        Some(player_uuid)
    );
    assert_eq!(entity.last_hurt_by_player_memory_time(), 0);

    entity.tick_living_state();

    assert!(entity.living_base().last_hurt_by_player_uuid().is_none());
    assert_eq!(entity.last_hurt_by_player_memory_time(), 0);
}

#[test]
fn living_tick_state_updates_swing_time() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.swing(InteractionHand::MainHand, false);
    assert_eq!(entity.living_swing_state().swing_time(), -1);

    entity.tick_living_state();

    let swing = entity.living_swing_state();
    assert!(swing.swinging());
    assert_eq!(swing.swing_time(), 0);
    assert_eq!(swing.attack_anim().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn current_swing_duration_uses_vanilla_dig_effects() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION);

    entity.set_mob_effect(vanilla_mob_effects::MINING_FATIGUE, 2);
    assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION + 6);

    entity.set_mob_effect(vanilla_mob_effects::HASTE, 1);
    assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION - 2);
}

#[test]
fn current_swing_duration_uses_held_item_component() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::WOODEN_SPEAR),
    );

    assert_eq!(entity.current_swing_duration(), 13);
}

#[test]
fn living_combat_memory_stores_and_expires_last_hurt_by_mob() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let attacker: SharedEntity = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
    entity.advance_tick_count();

    entity.set_last_hurt_by_mob(Some(&attacker));

    let Some(stored_attacker) = entity.last_hurt_by_mob() else {
        panic!("last hurt-by mob should be stored");
    };
    assert_eq!(stored_attacker.uuid(), attacker.uuid());
    assert_eq!(entity.last_hurt_by_mob_timestamp(), 1);

    entity.living_base().tick_living_combat_memory(101);
    assert!(entity.last_hurt_by_mob().is_some());

    entity.living_base().tick_living_combat_memory(102);
    assert!(entity.last_hurt_by_mob().is_none());
    assert_eq!(entity.last_hurt_by_mob_timestamp(), 102);
}

#[test]
fn living_combat_memory_clears_dead_last_hurt_mob() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let target = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
    let target_entity: SharedEntity = target.clone();

    entity.set_last_hurt_mob(Some(&target_entity));
    assert!(entity.last_hurt_mob().is_some());
    assert_eq!(entity.last_hurt_mob_timestamp(), 0);

    target.set_health(0.0);
    entity.living_base().tick_living_combat_memory(1);

    assert!(entity.last_hurt_mob().is_none());
    assert_eq!(entity.last_hurt_mob_timestamp(), 1);
}

#[test]
fn living_death_loot_table_uses_default_and_custom_mob_tables() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let Some(default_table) = pig.death_loot_table() else {
        panic!("pig should resolve its default entity loot table");
    };
    assert_eq!(&default_table.key, &vanilla_loot_tables::ENTITIES_PIG.key);

    pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/cow")));
    let Some(custom_table) = pig.death_loot_table() else {
        panic!("custom cow loot table should resolve");
    };
    assert_eq!(&custom_table.key, &vanilla_loot_tables::ENTITIES_COW.key);
    assert_eq!(LivingEntity::death_loot_table_seed(&pig), 0);

    pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/not_real")));
    assert!(pig.death_loot_table().is_none());
}

#[test]
fn closest_open_space_direction_matches_vanilla_order_on_ties() {
    assert_eq!(
        closest_direction_with_blocked_neighbors(DVec3::splat(0.5), &[]),
        Direction::North
    );
}

#[test]
fn closest_open_space_direction_skips_full_collision_neighbors() {
    assert_eq!(
        closest_direction_with_blocked_neighbors(DVec3::new(0.3, 0.5, 0.7), &[Direction::South]),
        Direction::West
    );
    assert_eq!(
        closest_direction_with_blocked_neighbors(
            DVec3::new(0.3, 0.2, 0.7),
            &[
                Direction::North,
                Direction::South,
                Direction::West,
                Direction::East,
            ],
        ),
        Direction::Up
    );
}

#[test]
fn resolved_movement_application_matches_vanilla_threshold() {
    assert!(should_apply_resolved_movement(DVec3::ZERO, DVec3::ZERO));
    assert!(should_apply_resolved_movement(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(1.0e-3, 0.0, 0.0)
    ));
    assert!(!should_apply_resolved_movement(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::ZERO
    ));
}

#[test]
fn move_without_physics_returns_none_when_position_commit_rejects() {
    init_test_registry();
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);
    entity.set_no_physics(true);
    entity.set_level_callback(Arc::new(CommitRejectingCallback {
        entity_id: entity.id(),
    }));

    let result = entity.move_without_physics(DVec3::new(1.0, 0.0, 0.0));

    assert!(result.is_none());
    assert_vec3_close(entity.position(), DVec3::ZERO);
}

#[test]
fn fall_damage_reset_clip_target_matches_vanilla_thresholds() {
    let position = DVec3::new(1.0, 2.0, 3.0);

    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(1.0, 0.0, 0.0), 0.0),
        None
    );
    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(0.999, 0.0, 0.0), 2.0),
        None
    );
    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(1.0, 0.0, 0.0), 2.0),
        Some(DVec3::new(2.0, 2.0, 3.0))
    );
    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(10.0, 0.0, 0.0), 2.0),
        Some(DVec3::new(9.0, 2.0, 3.0))
    );
}

#[test]
fn input_vector_ignores_tiny_input_like_vanilla() {
    assert_vec3_close(
        get_input_vector(DVec3::new(1.0E-4, 0.0, 0.0), 0.02, 0.0),
        DVec3::ZERO,
    );
}

#[test]
fn input_vector_normalizes_large_input_and_rotates_by_yaw() {
    assert_vec3_close(
        get_input_vector(DVec3::new(2.0, 0.0, 0.0), 0.5, 0.0),
        DVec3::new(0.5, 0.0, 0.0),
    );
    assert_vec3_close(
        get_input_vector(DVec3::new(0.0, 0.0, 1.0), 0.5, 90.0),
        DVec3::new(-0.5, 0.0, 0.0),
    );
}

#[test]
fn look_angle_matches_vanilla_view_vector_axes() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    entity.set_rotation((0.0, 0.0));
    assert_vec3_close(entity.look_angle(), DVec3::new(0.0, 0.0, 1.0));

    entity.set_rotation((90.0, 0.0));
    assert_vec3_close(entity.look_angle(), DVec3::new(-1.0, 0.0, 0.0));

    entity.set_rotation((0.0, 90.0));
    assert_vec3_close(entity.look_angle(), DVec3::new(0.0, -1.0, 0.0));
}

#[test]
fn fall_flying_movement_applies_vanilla_gravity_lift_and_drag() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_rotation((0.0, 0.0));

    assert_vec3_close(
        entity.update_fall_flying_movement(DVec3::ZERO),
        DVec3::new(
            0.0,
            -0.018 * f64::from(0.98_f32),
            0.0018 * f64::from(0.99_f32),
        ),
    );
}

#[test]
fn fall_flying_movement_converts_upward_pitch_to_lift() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_rotation((0.0, -45.0));

    let movement = entity.update_fall_flying_movement(DVec3::new(0.0, -0.2, 0.4));

    assert!(movement.y > -0.2);
    assert!(movement.z > 0.0);
}

#[test]
fn fall_flying_collision_damage_matches_vanilla_threshold() {
    assert!(fall_flying_collision_damage(1.0, 0.8) <= 0.0);
    assert!((fall_flying_collision_damage(1.0, 0.6) - 1.0).abs() < f32::EPSILON);
}

#[test]
fn in_wall_eye_box_requires_suffocating_state_and_shape_overlap() {
    init_test_registry();
    init_behaviors();
    let pos = BlockPos::ZERO;
    let level = EmptyTestLevel;
    let inside_box = WorldAabb::new(0.1, 0.5, 0.1, 0.9, 0.500_001, 0.9);
    let outside_box = WorldAabb::new(1.1, 0.5, 0.1, 1.9, 0.500_001, 0.9);

    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let glass = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::GLASS);
    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);

    assert!(block_state_suffocates_eye_box(
        stone, &level, pos, inside_box
    ));
    assert!(!block_state_suffocates_eye_box(
        glass, &level, pos, inside_box
    ));
    assert!(!block_state_suffocates_eye_box(
        air, &level, pos, inside_box
    ));
    assert!(!block_state_suffocates_eye_box(
        stone,
        &level,
        pos,
        outside_box
    ));
}

#[test]
fn fall_flying_free_fall_interval_matches_vanilla_cadence() {
    assert_eq!(fall_flying_free_fall_interval(8), None);
    assert_eq!(fall_flying_free_fall_interval(9), Some(1));
    assert_eq!(fall_flying_free_fall_interval(19), Some(2));
}

#[test]
fn jump_boost_power_uses_active_effect_amplifier() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(entity.get_jump_boost_power().abs() < f32::EPSILON);

    entity.set_mob_effect(vanilla_mob_effects::JUMP_BOOST, 2);

    assert!((entity.get_jump_boost_power() - 0.3).abs() < f32::EPSILON);
}

#[test]
fn levitation_travel_uses_active_effect_amplifier() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(entity.levitation_travel_y_delta(-0.2).is_none());

    entity.set_mob_effect(vanilla_mob_effects::LEVITATION, 1);

    assert!((entity.levitation_travel_y_delta(-0.2).unwrap_or(0.0) - 0.06).abs() < f64::EPSILON);
}

#[test]
fn slow_falling_caps_effective_gravity_only_while_falling() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_mob_effect_active(vanilla_mob_effects::SLOW_FALLING, true);
    entity.set_velocity(DVec3::new(0.0, -0.1, 0.0));

    assert!((entity.get_effective_gravity() - 0.01).abs() < f64::EPSILON);

    entity.set_velocity(DVec3::new(0.0, 0.1, 0.0));

    assert!((entity.get_effective_gravity() - entity.get_gravity()).abs() < f64::EPSILON);
}

#[test]
fn fall_distance_accumulation_clamps_like_vanilla() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_fall_distance(2.0);
    entity.set_velocity(DVec3::new(0.0, -0.4, 0.0));

    entity.check_fall_distance_accumulation();

    assert!((entity.fall_distance() - 1.0).abs() < f64::EPSILON);

    entity.set_fall_distance(2.0);
    entity.set_velocity(DVec3::new(0.0, -0.6, 0.0));

    entity.check_fall_distance_accumulation();

    assert!((entity.fall_distance() - 2.0).abs() < f64::EPSILON);
}

#[test]
fn can_glide_using_matches_vanilla_component_gate() {
    init_test_registry();
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
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert_f32_close(entity.get_armor_cover_percentage(), 0.0);

    entity.equip(EquipmentSlot::Head, ItemStack::new(&vanilla_items::STONE));
    entity.equip(EquipmentSlot::Feet, ItemStack::new(&vanilla_items::STONE));

    assert_f32_close(entity.get_armor_cover_percentage(), 0.5);
}

#[test]
fn living_visibility_percent_uses_discrete_and_invisible_scaling() {
    init_test_registry();
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
    init_test_registry();
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
    init_test_registry();
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
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity.equip(
        EquipmentSlot::Body,
        ItemStack::new(&vanilla_items::LEATHER_HORSE_ARMOR),
    );

    assert!(!entity.default_living_can_freeze());
}

#[test]
fn living_freeze_immunity_ignores_non_armor_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(entity.default_living_can_freeze());
}

#[test]
fn living_freezing_decays_when_not_in_powder_snow() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_ticks_frozen(10);

    entity.tick_freezing();

    assert_eq!(entity.ticks_frozen(), 8);
}

#[test]
fn living_freezing_keeps_ticks_while_in_powder_snow() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_ticks_frozen(10);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);

    entity.tick_freezing();

    assert_eq!(entity.ticks_frozen(), 11);
}

#[test]
fn living_freezing_adds_powder_snow_speed_modifier() {
    init_test_registry();
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
    init_test_registry();
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
    init_test_registry();
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
    init_test_registry();
    init_behaviors();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
    entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
    for _ in 0..40 {
        entity.advance_tick_count();
    }

    entity.default_ai_step();

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
    init_test_registry();
    let entity =
        LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::BLAZE);

    assert!(entity.hurt(
        test_world(),
        &DamageSource::environment(&vanilla_damage_types::FREEZE),
        1.0,
    ));

    assert_f32_close(entity.get_health(), 15.0);
}

#[test]
fn living_powder_snow_walkability_uses_feet_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(!entity.default_living_can_walk_on_powder_snow());

    entity.equip(
        EquipmentSlot::Feet,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(entity.default_living_can_walk_on_powder_snow());
}

#[test]
fn living_powder_snow_walkability_ignores_non_feet_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(!entity.default_living_can_walk_on_powder_snow());
}

#[test]
fn default_can_glide_uses_living_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(false);

    assert!(!entity.can_glide());

    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));

    assert!(entity.can_glide());
}

#[test]
fn try_to_start_fall_flying_uses_vanilla_glider_gate() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);

    assert!(entity.try_to_start_fall_flying());
    assert!(entity.is_fall_flying());
}

#[test]
fn try_to_start_fall_flying_rejects_levitation() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);
    entity.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);

    assert!(!entity.try_to_start_fall_flying());
    assert!(!entity.is_fall_flying());
}

#[test]
fn update_fall_flying_damages_glider_every_second_event_interval() {
    init_test_registry();
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
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_fall_flying(true);

    entity.update_fall_flying();

    assert!(!entity.is_fall_flying());
}

#[test]
fn fall_damage_sound_selects_vanilla_small_and_big_sounds() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert_eq!(
        entity.fall_damage_sound(4),
        &sound_events::ENTITY_GENERIC_SMALL_FALL
    );
    assert_eq!(
        entity.fall_damage_sound(5),
        &sound_events::ENTITY_GENERIC_BIG_FALL
    );
}

#[test]
fn living_fall_damage_uses_shared_damage_path_from_entity_dispatch() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
        .with_entity_type(&vanilla_entities::PIG);

    assert!(entity.cause_fall_damage(
        8.0,
        1.0,
        &DamageSource::environment(&vanilla_damage_types::FALL),
    ));

    assert_f32_close(entity.get_health(), 15.0);
}

#[test]
fn living_fall_damage_caps_distance_from_current_impulse() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());

    entity.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, 4.0, 0.0));

    assert!(entity.cause_fall_damage(
        8.0,
        1.0,
        &DamageSource::environment(&vanilla_damage_types::FALL),
    ));

    assert_f32_close(entity.get_health(), 19.0);
    assert!(!entity.is_ignoring_fall_damage_from_current_impulse());
}

#[test]
fn living_fall_damage_resets_current_impulse_when_landing_above_impact() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, -1.0, 0.0));

    assert!(!entity.cause_fall_damage(
        8.0,
        1.0,
        &DamageSource::environment(&vanilla_damage_types::FALL),
    ));

    assert_f32_close(entity.get_health(), 20.0);
    assert!(!entity.is_ignoring_fall_damage_from_current_impulse());
}

#[test]
fn stop_fall_flying_toggles_shared_state_back_to_false() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_fall_flying(true);

    entity.stop_fall_flying();

    assert!(!entity.is_fall_flying());
}

#[test]
fn fluid_falling_adjustment_matches_vanilla_special_falling_case() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    let movement =
        entity.get_fluid_falling_adjusted_movement(0.16, true, DVec3::new(1.0, 0.01, 1.0));

    assert_vec3_close(movement, DVec3::new(1.0, -0.003, 1.0));
}

#[test]
fn fluid_falling_adjustment_is_skipped_while_sprinting() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_sprinting(true);

    let movement =
        entity.get_fluid_falling_adjusted_movement(0.16, true, DVec3::new(1.0, 0.01, 1.0));

    assert_vec3_close(movement, DVec3::new(1.0, 0.01, 1.0));
}

#[test]
fn water_float_while_ridden_uses_vanilla_entity_type_tag_and_threshold() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true)
        .with_entity_type(&vanilla_entities::HORSE)
        .with_vehicle();

    entity.float_in_water_while_ridden();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, f64::from(0.04_f32), 0.0));
}

#[test]
fn water_float_while_ridden_ignores_non_vehicle_tagged_entity() {
    init_test_registry();
    let entity =
        LivingFluidTestEntity::new(0.5, 0.0, true).with_entity_type(&vanilla_entities::HORSE);

    entity.float_in_water_while_ridden();

    assert_vec3_close(entity.velocity(), DVec3::ZERO);
}

#[test]
fn inside_bubble_column_pushes_up_and_resets_fall_distance() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, 0.68, 0.2));
    entity.set_fall_distance(4.0);

    entity.on_inside_bubble_column(false);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.7, 0.2));
    assert_f64_close(entity.fall_distance(), 0.0);
}

#[test]
fn inside_bubble_column_drags_down_and_resets_fall_distance() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, -0.28, 0.2));
    entity.set_fall_distance(4.0);

    entity.on_inside_bubble_column(true);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, -0.3, 0.2));
    assert_f64_close(entity.fall_distance(), 0.0);
}

#[test]
fn above_bubble_column_uses_vanilla_stronger_velocity_limits() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, 1.75, 0.2));
    entity.set_fall_distance(4.0);

    entity.on_above_bubble_column(false, BlockPos::ZERO);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 1.8, 0.2));
    assert_f64_close(entity.fall_distance(), 4.0);
}

#[test]
fn above_bubble_column_drag_down_uses_vanilla_stronger_velocity_limit() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, -0.88, 0.2));

    entity.on_above_bubble_column(true, BlockPos::ZERO);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, -0.9, 0.2));
}

#[test]
fn flying_players_ignore_bubble_column_entity_hooks() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_flying_player();
    let velocity = DVec3::new(0.1, 0.2, 0.3);
    entity.set_velocity(velocity);
    entity.set_fall_distance(4.0);

    entity.on_inside_bubble_column(false);
    entity.on_above_bubble_column(false, BlockPos::ZERO);

    assert_vec3_close(entity.velocity(), velocity);
    assert_f64_close(entity.fall_distance(), 4.0);
}

#[test]
fn dolphins_grace_water_travel_hook_uses_active_mob_effect_state() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true);

    assert!(!entity.has_dolphins_grace());
    entity.set_mob_effect_active(vanilla_mob_effects::DOLPHINS_GRACE, true);
    assert!(entity.has_dolphins_grace());
}

#[test]
fn living_air_supply_decrements_while_eye_in_water() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

    entity.set_air_supply(entity.max_air_supply());
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 1);
}

#[test]
fn living_air_supply_drowning_damage_resets_air() {
    init_test_registry();
    let entity =
        LivingFluidTestEntity::new_in_world(0.5, 0.0, true, test_world()).with_eye_in_water();

    entity.set_air_supply(-19);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), 0);
    assert_f32_close(entity.get_health(), 18.0);
}

#[test]
fn water_breathing_refills_air_underwater() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.set_mob_effect_active(vanilla_mob_effects::WATER_BREATHING, true);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
}

#[test]
fn breath_of_the_nautilus_prevents_drowning_without_refilling_air() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.set_mob_effect_active(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS, true);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 8);
    assert_f32_close(entity.get_health(), 20.0);
}

#[test]
fn entity_type_can_breathe_underwater_refills_air() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true)
        .with_eye_in_water()
        .with_entity_type(&vanilla_entities::ZOMBIE);

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
}

#[test]
fn living_air_supply_refills_out_of_water() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
}

#[test]
fn living_base_tick_damages_entities_in_wall() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
        .with_in_wall_for_base_tick();

    entity.base_tick_living_entity();

    assert_f32_close(entity.get_health(), 19.0);
}

#[test]
fn living_environmental_damage_applies_in_wall_before_drowning() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.5, 0.0, true, test_world())
        .with_eye_in_water()
        .with_in_wall_for_base_tick();

    entity.set_air_supply(-19);
    entity.tick_living_environmental_damage();

    assert_eq!(
        entity.damage_type_keys(),
        vec![
            vanilla_damage_types::IN_WALL.key.clone(),
            vanilla_damage_types::DROWN.key.clone(),
        ]
    );
}

#[test]
fn living_base_tick_skips_in_wall_damage_while_sleeping() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_in_wall_for_base_tick();
    entity.set_sleeping_pos(BlockPos::ZERO);

    entity.base_tick_living_entity();

    assert_f32_close(entity.get_health(), 20.0);
}

#[test]
fn generic_living_hurt_applies_health_damage() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_f32_close(entity.get_health(), 16.0);
}

#[test]
fn generic_living_hurt_ignores_fire_damage_with_fire_resistance() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_mob_effect(vanilla_mob_effects::FIRE_RESISTANCE, 0);
    let source = DamageSource::environment(&vanilla_damage_types::LAVA);

    assert!(!entity.hurt(test_world(), &source, 4.0));

    assert_f32_close(entity.get_health(), 20.0);
}

#[test]
fn generic_living_hurt_processes_default_death_once() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world()).with_health(3.0);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert!(entity.hurt(test_world(), &source, 4.0));
    assert_f32_close(entity.get_health(), 0.0);
    assert_eq!(entity.pose(), EntityPose::Dying);
    assert!(!entity.hurt(test_world(), &source, 1.0));
}

#[test]
fn generic_living_hurt_applies_armor_and_absorption() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    {
        let mut attributes = entity.attributes().lock();
        attributes.set_base_value(vanilla_attributes::ARMOR, 20.0);
        attributes.set_base_value(vanilla_attributes::MAX_ABSORPTION, 3.0);
    }
    entity.set_absorption_amount(3.0);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    assert_f32_close(entity.get_health(), 19.0);
    assert_f32_close(entity.get_absorption_amount(), 0.0);
}

#[test]
fn generic_living_hurt_applies_resistance() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_mob_effect(vanilla_mob_effects::RESISTANCE, 0);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    assert_f32_close(entity.get_health(), 12.0);
}

#[test]
fn damage_reductions_use_victim_attached_world() {
    init_test_registry();
    let attached_world = cross_world_damage_test_world();
    let explicit_world = test_world();
    assert!(!Arc::ptr_eq(attached_world, explicit_world));

    let attacker_id = 1_750_001;
    let attacker = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        attacker_id,
        DVec3::ZERO,
        Arc::downgrade(attached_world),
    ));
    let mut mace = ItemStack::new(&vanilla_items::MACE);
    mace.set_enchantments(&[(Identifier::vanilla_static("breach"), 4)], false);
    attacker
        .living_base()
        .equipment()
        .lock()
        .set(EquipmentSlot::MainHand, mace);
    let attacker: SharedEntity = attacker;
    let registration = attached_world
        .entity_manager()
        .add_live_entity(attacker, EntityOwnership::External);
    assert!(registration.is_ok());

    let victim = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, attached_world);
    victim
        .attributes()
        .lock()
        .set_base_value(vanilla_attributes::ARMOR, 20.0);
    let source = DamageSource::environment(&vanilla_damage_types::MOB_ATTACK)
        .with_causing_entity(attacker_id)
        .with_direct_entity(attacker_id);

    let damage_applied = victim.hurt(explicit_world, &source, 10.0);
    let health = victim.get_health();
    let removed = attached_world
        .entity_manager()
        .remove_live_entity(attacker_id, RemovalReason::Discarded);

    assert!(removed.is_some());
    assert!(damage_applied);
    assert_f32_close(health, 10.0);
}

#[test]
fn generic_living_hurt_applies_damage_protection_enchantments() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
    let mut boots = ItemStack::new(&vanilla_items::DIAMOND_BOOTS);
    boots.set_enchantments(&[(Identifier::vanilla_static("protection"), 4)], false);
    entity.equip(EquipmentSlot::Feet, boots);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    let expected_health = 20.0_f32 - 10.0_f32 * (1.0 - 4.0_f32 / 25.0);
    assert_eq!(entity.get_health().to_bits(), expected_health.to_bits());
}

#[test]
fn generic_living_default_does_not_damage_armor_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::Chest,
        ItemStack::new(&vanilla_items::DIAMOND_CHESTPLATE),
    );
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    entity.with_equipment_slot(EquipmentSlot::Chest, &mut |item| {
        assert_eq!(item.get_damage_value(), 0);
    });
}

#[test]
fn generic_living_hurt_applies_source_position_knockback() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(true);
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_source_position(DVec3::new(1.0, 0.0, 0.0));

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(-DAMAGE_KNOCKBACK_POWER, 0.4, 0.0),
    );
    assert!(entity.needs_velocity_sync());
}

#[test]
fn try_as_dyn_exposes_living_entity_behavior() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let entity_ref: &dyn Entity = &entity;
    let Some(living) = entity_ref.as_living_entity() else {
        panic!("living test entity did not expose LivingEntity behavior");
    };

    assert_f32_close(living.get_health(), 20.0);

    let non_living = PushableTestEntity::shared(2, DVec3::ZERO);
    assert!(non_living.as_living_entity().is_none());
}

#[test]
fn head_yaw_uses_living_head_rotation_only() {
    init_test_registry();
    let living = LivingFluidTestEntity::new(0.0, 0.0, true);
    living.set_rotation((35.0, 0.0));
    living.set_y_head_rot(120.0);

    assert_f32_close(Entity::head_yaw(&living), 120.0);

    let non_living = PushableTestEntity::shared(2, DVec3::ZERO);
    non_living.set_rotation((35.0, 0.0));
    assert_f32_close(non_living.head_yaw(), 0.0);
}

#[test]
fn living_equipment_attribute_modifiers_refresh_for_slot() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let (base_armor, base_toughness) = {
        let attributes = entity.attributes().lock();
        (
            attributes.required_value(vanilla_attributes::ARMOR),
            attributes.required_value(vanilla_attributes::ARMOR_TOUGHNESS),
        )
    };

    entity.equip(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );
    LivingEntity::refresh_equipment_attribute_modifiers(&entity, EquipmentSlot::Head);

    {
        let attributes = entity.attributes().lock();
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::ARMOR)
                .to_bits(),
            (base_armor + 3.0).to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::ARMOR_TOUGHNESS)
                .to_bits(),
            (base_toughness + 2.0).to_bits()
        );
    }

    entity.equip(EquipmentSlot::Head, ItemStack::empty());
    LivingEntity::refresh_equipment_attribute_modifiers(&entity, EquipmentSlot::Head);

    let attributes = entity.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::ARMOR)
            .to_bits(),
        base_armor.to_bits()
    );
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::ARMOR_TOUGHNESS)
            .to_bits(),
        base_toughness.to_bits()
    );
}

#[test]
fn generic_living_hurt_respects_no_knockback_damage_tag() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(true);
    entity.set_velocity(DVec3::new(0.2, 0.3, -0.1));
    let initial_velocity = entity.velocity();
    let source = DamageSource::environment(&vanilla_damage_types::DROWN)
        .with_source_position(DVec3::new(1.0, 0.0, 0.0));

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_vec3_close(entity.velocity(), initial_velocity);
    assert!(!entity.needs_velocity_sync());
}

#[test]
fn generic_living_hurt_scales_knockback_by_resistance() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(true);
    entity
        .attributes()
        .lock()
        .set_base_value(vanilla_attributes::KNOCKBACK_RESISTANCE, 0.5);
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_source_position(DVec3::new(1.0, 0.0, 0.0));

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(
            -DAMAGE_KNOCKBACK_POWER * 0.5,
            DAMAGE_KNOCKBACK_POWER * 0.5,
            0.0,
        ),
    );
}

#[test]
fn jump_from_ground_uses_jump_strength_and_marks_velocity_sync() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);

    entity.jump_from_ground();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
    assert!(entity.needs_velocity_sync());
}

#[test]
fn sprint_jump_from_ground_adds_vanilla_horizontal_impulse() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
    entity.set_sprinting(true);
    entity.set_rotation((0.0, 0.0));

    entity.jump_from_ground();

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(0.0, jump_strength, f64::from(0.2_f32)),
    );
}

#[test]
fn living_jump_in_water_uses_fluid_jump_impulse_without_cooldown() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true);
    entity.set_jumping(true);

    entity.handle_living_jump();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    assert_eq!(entity.no_jump_delay(), 0);
}

#[test]
fn living_jump_without_input_resets_jump_delay_like_vanilla() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_no_jump_delay(4);

    entity.handle_living_jump();

    assert_eq!(entity.no_jump_delay(), 0);
}

#[test]
fn living_ai_step_zeroes_tiny_player_velocity_like_vanilla() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.002, 0.002, 0.002));

    entity.apply_living_velocity_thresholds();

    assert_vec3_close(entity.velocity(), DVec3::ZERO);
}

#[test]
fn living_ai_step_keeps_player_horizontal_velocity_above_combined_threshold() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let velocity = DVec3::new(0.002, 0.003, 0.0025);
    entity.set_velocity(velocity);

    entity.apply_living_velocity_thresholds();

    assert_vec3_close(entity.velocity(), velocity);
}

#[test]
fn default_ai_step_resets_idle_jump_delay_and_dampens_input_before_travel() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_no_jump_delay(2);
    entity.set_travel_input(LivingTravelInput::new(1.0, 0.5, -1.0));

    assert!(entity.default_ai_step().is_none());

    assert_eq!(entity.no_jump_delay(), 0);
    assert_eq!(
        entity.travel_input(),
        LivingTravelInput::new(0.98, 0.5, -0.98)
    );
}

#[test]
fn default_ai_step_resets_fall_distance_for_slow_falling_and_levitation() {
    init_test_registry();

    let slow_falling = LivingFluidTestEntity::new(0.0, 0.0, true);
    slow_falling.set_fall_distance(7.0);
    slow_falling.set_mob_effect_active(vanilla_mob_effects::SLOW_FALLING, true);
    slow_falling.default_ai_step();

    assert_f64_close(slow_falling.fall_distance(), 0.0);

    let levitating = LivingFluidTestEntity::new(0.0, 0.0, true);
    levitating.set_fall_distance(7.0);
    levitating.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);
    levitating.default_ai_step();

    assert_f64_close(levitating.fall_distance(), 0.0);
}

#[test]
fn default_ai_step_jumps_from_ground_and_sets_vanilla_cooldown() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
    entity.set_on_ground(true);
    entity.set_jumping(true);

    assert!(entity.default_ai_step().is_none());

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
    assert_eq!(entity.no_jump_delay(), 10);
    assert!(entity.needs_velocity_sync());
}

#[test]
fn living_travel_fluid_predicate_matches_vanilla_hooks() {
    init_test_registry();
    let water = FluidState::source(&vanilla_fluids::WATER);

    assert!(LivingFluidTestEntity::new(0.4, 0.0, true).should_travel_in_fluid(water));
    assert!(LivingFluidTestEntity::new(0.0, 0.4, true).should_travel_in_fluid(water));
    assert!(!LivingFluidTestEntity::new(0.0, 0.0, true).should_travel_in_fluid(water));
    assert!(!LivingFluidTestEntity::new(0.4, 0.0, false).should_travel_in_fluid(water));
    assert!(
        !LivingFluidTestEntity::new(0.4, 0.0, true)
            .with_standing_on_fluid()
            .should_travel_in_fluid(water)
    );
}

#[test]
fn open_trapdoor_matches_ladder_facing_for_climbable() {
    init_test_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, true)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);

    assert!(trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn closed_trapdoor_is_not_usable_as_ladder() {
    init_test_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);

    assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn trapdoor_ladder_facing_must_match() {
    init_test_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, true)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::South);

    assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn vertical_collision_state_update_matches_vanilla_authority_gate() {
    assert!(
        EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.0, -0.1, 0.0), false)
            .refreshes_state()
    );
    assert!(EntityVerticalMovementStateUpdate::for_move(DVec3::ZERO, true).refreshes_state());
    assert!(
        !EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.1, 0.0, 0.0), false)
            .refreshes_state()
    );
}

#[test]
fn push_impulse_updates_velocity_and_marks_sync() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    entity.push_impulse(DVec3::new(0.1, 0.2, 0.3));

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
    assert!(entity.needs_velocity_sync());

    entity.clear_velocity_sync();
    entity.push_impulse(DVec3::new(f64::INFINITY, 0.0, 0.0));

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
    assert!(!entity.needs_velocity_sync());
}

#[test]
fn default_below_world_hook_discards_entity() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    entity.on_below_world();

    assert!(entity.is_removed());
}

#[test]
fn base_entity_has_no_controlling_passenger() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    assert!(entity.controlling_passenger().is_none());
    assert!(!entity.has_controlling_passenger());
}

#[test]
fn start_riding_entities_links_passenger_and_vehicle() {
    init_test_registry();

    let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
    let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);

    assert!(start_riding_entities(&passenger, &vehicle));

    assert!(passenger.is_passenger());
    assert_eq!(passenger.vehicle().map(|entity| entity.id()), Some(2));
    assert!(vehicle.has_passenger(passenger.as_ref()));
    assert_eq!(vehicle.first_passenger().map(|entity| entity.id()), Some(1));
    assert_eq!(passenger.pose(), EntityPose::Standing);
}

#[test]
fn transfer_leashables_to_holder_moves_valid_mobs() {
    init_test_registry();

    let old_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Weak::new(),
    ));
    let new_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(1.0, 0.0, 0.0),
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&old_holder));

    assert!(transfer_leashables_to_holder(
        vec![Arc::clone(&leashable)],
        &new_holder
    ));

    let Some(holder) = mob.leash_holder() else {
        panic!("transferred mob should stay leashed");
    };
    assert_eq!(holder.id(), new_holder.id());
}

#[test]
fn transfer_leashables_to_holder_skips_mobs_outside_snap_distance() {
    init_test_registry();

    let old_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Weak::new(),
    ));
    let new_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(20.0, 0.0, 0.0),
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&old_holder));

    assert!(!transfer_leashables_to_holder(
        vec![Arc::clone(&leashable)],
        &new_holder
    ));

    let Some(holder) = mob.leash_holder() else {
        panic!("untransferred mob should stay leashed");
    };
    assert_eq!(holder.id(), old_holder.id());
}

#[test]
fn set_leashed_to_notifies_replaced_holder() {
    init_test_registry();

    let old_holder_typed = LeashNotificationTestEntity::new(1);
    let old_holder: SharedEntity = old_holder_typed.clone();
    let new_holder_typed = LeashNotificationTestEntity::new(2);
    let new_holder: SharedEntity = new_holder_typed.clone();
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::ZERO,
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };

    assert!(mob.set_leashed_to(&old_holder));
    assert!(mob.set_leashed_to(&new_holder));

    assert_eq!(old_holder_typed.removed_notifications(), vec![3]);
    assert!(new_holder_typed.removed_notifications().is_empty());
}

#[test]
fn tick_leash_notifies_live_holder() {
    init_test_registry();

    let holder_typed = LeashNotificationTestEntity::new(1);
    let holder: SharedEntity = holder_typed.clone();
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::ZERO,
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&holder));

    mob.tick_leash();

    assert_eq!(holder_typed.holder_notifications(), vec![3]);
    assert!(mob.is_leashed());
    assert!(holder_typed.removed_notifications().is_empty());
}

#[test]
fn tick_leash_snaps_live_holder_past_snap_distance() {
    init_test_registry();

    let holder_typed = LeashNotificationTestEntity::with_position(1, DVec3::new(13.0, 0.0, 0.0));
    let holder: SharedEntity = holder_typed.clone();
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::ZERO,
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&holder));

    mob.tick_leash();

    assert_eq!(holder_typed.holder_notifications(), vec![3]);
    assert_eq!(holder_typed.removed_notifications(), vec![3]);
    assert!(!mob.is_leashed());
}

#[test]
fn start_riding_entities_respects_boarding_cooldown() {
    init_test_registry();

    let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
    let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);
    passenger.base().set_boarding_cooldown(2);

    assert!(!start_riding_entities(&passenger, &vehicle));
    assert!(!passenger.is_passenger());
    assert!(!vehicle.is_vehicle());
}

#[test]
fn start_riding_entities_rejects_vehicle_cycles() {
    init_test_registry();

    let root = PushableTestEntity::shared(1, DVec3::ZERO);
    let child = PushableTestEntity::shared(2, DVec3::ZERO);
    EntityBase::restore_passenger_relationship(&root, &child);

    assert!(!start_riding_entities(&root, &child));
    assert_eq!(child.vehicle().map(|entity| entity.id()), Some(1));
    assert_eq!(root.first_passenger().map(|entity| entity.id()), Some(2));
}

#[test]
fn start_riding_entities_inserts_player_passenger_first() {
    init_test_registry();

    let vehicle = MultiPassengerTestEntity::shared(1);
    let mob_passenger = PushableTestEntity::shared(2, DVec3::ZERO);
    let player_passenger =
        KnownMovementTestEntity::shared(3, &vanilla_entities::PLAYER, DVec3::ZERO, DVec3::ZERO);

    assert!(start_riding_entities(&mob_passenger, &vehicle));
    assert!(start_riding_entities(&player_passenger, &vehicle));

    let passenger_ids = vehicle
        .passengers()
        .into_iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    assert_eq!(passenger_ids, vec![3, 2]);
}

#[test]
fn controlled_vehicle_uses_player_known_movement_and_speed() {
    let player_movement = DVec3::new(0.25, 0.0, -0.5);
    let player_speed = DVec3::new(0.5, 0.0, -1.0);
    let controller = KnownMovementTestEntity::shared(
        1,
        &vanilla_entities::PLAYER,
        player_movement,
        player_speed,
    );
    let vehicle = ControlledVehicleTestEntity::shared(2, Some(controller));

    assert!(vehicle.uses_client_movement_packets());
    assert!(!vehicle.is_server_driven_movement());
    assert!(!vehicle.can_simulate_movement());
    assert!(!vehicle.is_effective_ai());

    vehicle.set_velocity(DVec3::new(4.0, 0.0, 4.0));
    vehicle.base().advance_base_tick_state();
    vehicle.base().set_position_local(DVec3::new(2.0, 0.0, 0.0));
    vehicle.base().advance_base_tick_state();

    assert!(vehicle.has_controlling_passenger());
    assert_vec3_close(vehicle.known_movement(), player_movement);
    assert_vec3_close(vehicle.known_speed(), player_speed);

    vehicle.set_removed(RemovalReason::Discarded);

    assert_vec3_close(vehicle.known_movement(), DVec3::new(4.0, 0.0, 4.0));
    assert_vec3_close(vehicle.known_speed(), DVec3::new(2.0, 0.0, 0.0));
}

#[test]
fn controlled_vehicle_returns_direct_controlled_vehicle_not_root_vehicle() {
    init_test_registry();

    let passenger =
        KnownMovementTestEntity::shared(1, &vanilla_entities::PLAYER, DVec3::ZERO, DVec3::ZERO);
    let vehicle = ControlledVehicleTestEntity::shared(2, Some(Arc::clone(&passenger)));
    let root_vehicle = ControlledVehicleTestEntity::shared(3, None);

    assert!(start_riding_entities(&passenger, &vehicle));
    assert!(start_riding_entities(&vehicle, &root_vehicle));

    let Some(controlled_vehicle) = passenger.controlled_vehicle() else {
        panic!("passenger should directly control the middle vehicle");
    };
    let Some(root) = passenger.root_vehicle() else {
        panic!("passenger should have a root vehicle");
    };

    assert_eq!(controlled_vehicle.id(), vehicle.id());
    assert_eq!(root.id(), root_vehicle.id());
}

#[test]
fn controlled_vehicle_known_movement_falls_back_without_active_player_controller() {
    let non_player_controller = KnownMovementTestEntity::shared(
        1,
        &vanilla_entities::ZOMBIE,
        DVec3::new(0.25, 0.0, -0.5),
        DVec3::new(0.5, 0.0, -1.0),
    );
    let vehicle = ControlledVehicleTestEntity::shared(2, Some(non_player_controller));
    vehicle.set_velocity(DVec3::new(4.0, 0.0, 4.0));
    vehicle.base().advance_base_tick_state();
    vehicle.base().set_position_local(DVec3::new(2.0, 0.0, 0.0));
    vehicle.base().advance_base_tick_state();

    assert_vec3_close(vehicle.known_movement(), DVec3::new(4.0, 0.0, 4.0));
    assert_vec3_close(vehicle.known_speed(), DVec3::new(2.0, 0.0, 0.0));
}

#[test]
fn push_entity_separates_pushable_entities_like_vanilla() {
    let left = PushableTestEntity::shared(1, DVec3::ZERO);
    let right = PushableTestEntity::shared(2, DVec3::new(1.0, 0.0, 0.0));

    left.push_entity(right.as_ref());

    assert_vec3_close(left.velocity(), DVec3::new(-0.05, 0.0, 0.0));
    assert_vec3_close(right.velocity(), DVec3::new(0.05, 0.0, 0.0));
    assert!(left.needs_velocity_sync());
    assert!(right.needs_velocity_sync());
}
