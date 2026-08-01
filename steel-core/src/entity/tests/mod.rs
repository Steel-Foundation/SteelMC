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
use steel_utils::types::{Difficulty, InteractionHand};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, Identifier, SectionPos, WorldAabb, axis::Axis,
    block_util::FoundRectangle,
};
use text_components::{Modifier as _, TextComponent, format::Color, interactivity::ClickEvent};
use uuid::Uuid;

use crate::behavior::{BlockBehavior, blocks::WitherRoseBlock, init_behaviors};
use crate::chunk_saver::ChunkStorage;
use crate::entity::damage::DamageSource;
use crate::entity::entities::{ChestMinecartEntity, PigEntity};
use crate::entity::mob::Mob;
use crate::inventory::equipment::EquipmentSlot;
use crate::portal::PortalKind;
use crate::test_support::{
    cross_world_damage_test_world, fresh_test_world, insert_ready_full_chunk, test_world,
};
use crate::world::game_event::GameEventContext;
use crate::world::game_event::{GameEventListener, SharedGameEventListener};
use crate::world::{LevelReader, World};

use super::{
    ActiveMobEffect, AttributeModifier, AttributeModifierOperation, DAMAGE_KNOCKBACK_POWER,
    DEFAULT_SWING_DURATION, DEFAULT_TICKS_REQUIRED_TO_FREEZE, Entity, EntityBase,
    EntityFluidContact, EntityLevelCallback, EntityMoveError, EntityOwnership, EntitySyncedData,
    EntityVerticalMovementStateUpdate, InsideBlockEffectCollector, InsideBlockEffectType,
    LivingEntity, LivingEntityBase, LivingTravelInput, MobEffectInstance, RemovalReason,
    SPEED_MODIFIER_POWDER_SNOW_ID, SharedEntity, block_state_suffocates_eye_box,
    closest_open_space_direction, fall_damage_reset_clip_target, fall_flying_collision_damage,
    fall_flying_free_fall_interval, get_input_vector, indirect_passengers,
    passenger_transition_position, passenger_transition_rotation, remove_after_changing_dimensions,
    should_apply_entity_cramming_damage, should_apply_resolved_movement, start_riding_entities,
    transfer_leashables_to_holder, trapdoor_usable_as_ladder_state,
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
    init_test_registry();
    init_behaviors();
    let source_world_storage = fresh_test_world("kill_game_event_source");
    let target_world_storage = fresh_test_world("kill_game_event_target");
    let source_world = &source_world_storage;
    let target_world = &target_world_storage;
    assert!(!Arc::ptr_eq(source_world, target_world));
    let non_living_position = DVec3::new(0.25, 64.75, -0.125);
    let living_position = DVec3::new(1.25, 64.75, -0.125);
    let listener_position = DVec3::new(0.75, 64.75, -0.125);
    let listener_section = SectionPos::from_block_pos(BlockPos::from(listener_position));
    let listener_chunk = ChunkPos::new(listener_section.x(), listener_section.z());
    insert_ready_full_chunk(source_world, listener_chunk);
    insert_ready_full_chunk(target_world, listener_chunk);
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
    rejects_wither: bool,
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
            rejects_wither: false,
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

    const fn rejecting_wither(mut self) -> Self {
        self.rejects_wither = true;
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

    fn can_be_affected(&self, effect: &MobEffectInstance) -> bool {
        if self.rejects_wither && effect.effect() == vanilla_mob_effects::WITHER {
            return false;
        }
        self.default_can_be_affected(effect)
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

fn apply_wither_rose_effect(world: &Arc<World>, entity: &dyn Entity) {
    let behavior = WitherRoseBlock::new(&vanilla_blocks::WITHER_ROSE);
    behavior.entity_inside(
        vanilla_blocks::WITHER_ROSE.default_state(),
        world,
        BlockPos::ZERO,
        entity,
        &mut InsideBlockEffectCollector::new(),
        false,
    );
}

#[test]
fn wither_rose_effect_ticks_vanilla_wither_damage() {
    let world = test_world();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, world);

    apply_wither_rose_effect(world, &entity);

    let effect = entity
        .mob_effect(vanilla_mob_effects::WITHER)
        .expect("wither rose should apply Wither");
    assert_eq!(effect.duration(), 40);
    assert_eq!(effect.amplifier(), 0);

    entity.tick_mob_effects();

    assert_f32_close(entity.get_health(), 19.0);
    assert_eq!(
        entity.damage_type_keys(),
        vec![vanilla_damage_types::WITHER.key.clone()]
    );
    assert_eq!(
        entity
            .mob_effect(vanilla_mob_effects::WITHER)
            .expect("Wither should remain active")
            .duration(),
        39
    );
}

#[test]
fn wither_effect_only_damages_on_its_vanilla_interval() {
    let world = test_world();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, world);
    assert!(entity.add_mob_effect(MobEffectInstance::with_duration(
        vanilla_mob_effects::WITHER,
        39,
        0,
    )));

    entity.tick_mob_effects();

    assert_f32_close(entity.get_health(), 20.0);
    assert!(entity.damage_type_keys().is_empty());
    assert_eq!(
        entity
            .mob_effect(vanilla_mob_effects::WITHER)
            .expect("Wither should remain active")
            .duration(),
        38
    );
}

#[test]
fn wither_rose_respects_difficulty_invulnerability_and_effect_immunity() {
    let peaceful_world = fresh_test_world("wither_rose_peaceful");
    peaceful_world.set_difficulty(Difficulty::Peaceful);
    let peaceful_entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, &peaceful_world);
    apply_wither_rose_effect(&peaceful_world, &peaceful_entity);
    assert!(!peaceful_entity.has_mob_effect(vanilla_mob_effects::WITHER));

    let world = test_world();
    let invulnerable = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, world);
    invulnerable.set_invulnerable(true);
    apply_wither_rose_effect(world, &invulnerable);
    assert!(!invulnerable.has_mob_effect(vanilla_mob_effects::WITHER));

    let effect_immune =
        LivingFluidTestEntity::new_in_world(0.0, 0.0, true, world).rejecting_wither();
    apply_wither_rose_effect(world, &effect_immune);
    assert!(!effect_immune.has_mob_effect(vanilla_mob_effects::WITHER));
}

#[test]
fn default_mob_effect_eligibility_uses_vanilla_entity_type_tags() {
    init_test_registry();
    let silverfish =
        LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::SILVERFISH);
    assert!(
        !silverfish.can_be_affected(&MobEffectInstance::with_duration(
            vanilla_mob_effects::INFESTED,
            20,
            0,
        ))
    );

    let slime =
        LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::SLIME);
    assert!(!slime.can_be_affected(&MobEffectInstance::with_duration(
        vanilla_mob_effects::OOZING,
        20,
        0,
    )));

    let zombie =
        LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::ZOMBIE);
    assert!(!zombie.can_be_affected(&MobEffectInstance::with_duration(
        vanilla_mob_effects::POISON,
        20,
        0,
    )));
    assert!(!zombie.can_be_affected(&MobEffectInstance::with_duration(
        vanilla_mob_effects::REGENERATION,
        20,
        0,
    )));
    assert!(zombie.can_be_affected(&MobEffectInstance::with_duration(
        vanilla_mob_effects::WITHER,
        20,
        0,
    )));
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

mod damage;
mod equipment_and_freezing;
mod fall_and_fluids;
mod living_state;
mod movement;
mod portals;
mod riding_and_leashes;
mod travel;
