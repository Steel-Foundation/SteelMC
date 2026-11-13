use steel_math::{DEGREE_90, trig};
use steel_registry::data_components::vanilla_components::{DEATH_PROTECTION, USE_EFFECTS};
use steel_registry::entity_data::ParticleList;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{DyeColor, vanilla_custom_stats};
use steel_utils::locks::CheckedMutex;

use std::ops::Deref;

use steel_registry::vanilla_entity_data::LivingEntityData;

use super::*;
use crate::behavior::{BLOCK_BEHAVIORS, InventoryTickContext, MOB_EFFECT_BEHAVIORS};
use crate::entity::EntityProps;
use crate::entity::component::props::entity_props;
use crate::entity::component::{Components as _, Has};
use crate::entity::consume_effect::apply_consume_effect;
use crate::entity::damage::RecentDamageSource;
use crate::entity::entity::{
    compute_modified_friction, landing_particle_count, landing_particle_position,
};
use crate::entity::pushing::EntityPush;

mod damage;
mod dismount;
mod effects;
mod environment;
mod equipment;
mod movement;
mod sleeping;
mod travel;

pub use damage::{
    absorption_amount, actually_hurt, die, drop_custom_death_loot, handle_killing_blow, hurt,
    hurt_armor, hurt_broadcast_chunk, hurt_helmet, indicate_damage, is_always_experience_dropper,
    is_invulnerable_to, set_absorption_amount, should_drop_experience, should_drop_loot,
};
pub use dismount::dismount_poses;
pub use effects::can_be_affected;
pub use environment::{
    can_freeze, can_stand_on_fluid, is_affected_by_fluids, is_on_non_air_block_for_frost,
};
pub use equipment::{
    does_emit_equip_event, equip_sound, handle_extra_items_created_on_use, has_infinite_materials,
    interact_living_entity_with_equippable, is_blocking, is_using_item, on_equipped_item_broken,
    tick_equipment, with_equipment_slot, with_equipment_slot_mut,
};
pub use movement::{
    ai_step, block_speed_factor, can_glide, can_glide_using, cause_fall_damage, check_fall_damage,
    is_immobile, jump_from_ground, push_entities, ridden_input, ridden_speed, server_ai_step,
};
pub use sleeping::stop_sleeping;
pub use travel::{air_travel_vertical_friction, flying_speed, travel, water_slow_down};

const BED_SLEEP_POSITION_OFFSET: f64 = 0.125;
/// `LivingEntity.SLEEPING_DIMENSIONS`.
const SLEEPING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 0.2);

entity_props! {
    /// Per-type properties of a vanilla `LivingEntity`.
    pub struct LivingProps<E>: LivingHooks for Living
    where E: Parts,
    table LivingTable::LIVING,
    component LivingEntityBase {
        /// Current absorption hearts.
        fn absorption_amount() -> f32;
        /// Sets the absorption hearts.
        fn set_absorption_amount(amount: f32);
        /// Reacts to an equipped item breaking.
        fn on_equipped_item_broken(item: ItemRef, slot: EquipmentSlot);
        /// `LivingEntity.isAffectedByFluids`.
        fn is_affected_by_fluids() -> bool;
        /// `LivingEntity.indicateDamage`.
        fn indicate_damage(xd: f64, zd: f64);
        /// `LivingEntity.isUsingItem`.
        fn is_using_item() -> bool;
        /// `LivingEntity.getLuck`.
        fn luck() -> f32;
        /// `LivingEntity.hurtArmor`.
        fn hurt_armor(source: &DamageSource, damage: f32);
        /// `LivingEntity.hurtHelmet`, for damage that hits a worn helmet.
        fn hurt_helmet(source: &DamageSource, damage: f32);
        /// `LivingEntity.doesEmitEquipEvent`.
        fn does_emit_equip_event(slot: EquipmentSlot) -> bool;
        /// `LivingEntity.actuallyHurt`.
        fn actually_hurt(world: &World, source: &DamageSource, amount: f32);
        /// The chunk whose viewers receive this entity's hurt animation.
        fn hurt_broadcast_chunk() -> ChunkPos;
        /// `LivingEntity.die`.
        fn die(source: &DamageSource);
        /// Ticks equipment change detection.
        fn tick_equipment();
        /// Visits the item in `slot`.
        fn with_equipment_slot(slot: EquipmentSlot, visitor: &mut dyn FnMut(&ItemStack));
        /// Visits the item in `slot` mutably.
        fn with_equipment_slot_mut(slot: EquipmentSlot, visitor: &mut dyn FnMut(&mut ItemStack));
        /// Equips `player`'s held item onto this entity.
        fn interact_living_entity_with_equippable(player: &Player, hand: InteractionHand) -> InteractionResult;
        /// `LivingEntity.hasInfiniteMaterials`.
        fn has_infinite_materials() -> bool;
        /// `LivingEntity.handleExtraItemsCreatedOnUse`.
        fn handle_extra_items_created_on_use(extra: ItemStack);
        /// `LivingEntity.getDismountPoses`: the poses tried, in order, when
        /// leaving a vehicle.
        fn dismount_poses() -> &'static [EntityPose];
        /// `LivingEntity.stopSleeping`.
        fn stop_sleeping();
        /// `LivingEntity.getFlyingSpeed`.
        fn flying_speed() -> f32;
        /// `LivingEntity.getHurtSound`.
        fn hurt_sound(source: &DamageSource) -> Option<SoundEventRef>;
        /// `LivingEntity.getDeathSound`.
        fn death_sound() -> Option<SoundEventRef>;
        /// The chicken variant key for loot predicates, when this is a chicken.
        fn chicken_loot_variant() -> Option<&'static Identifier>;
        /// `LivingEntity.serverAiStep`.
        fn server_ai_step();
        /// `LivingEntity.canBeAffected`.
        fn can_be_affected(effect: &MobEffectInstance) -> bool;
        /// `LivingEntity.canStandOnFluid`.
        fn can_stand_on_fluid(fluid_state: FluidState) -> bool;
        /// Whether Frost Walker may act under this entity.
        fn is_on_non_air_block_for_frost() -> bool;
        /// All attributes clients see, for spawning.
        fn pack_syncable_attributes() -> Vec<AttributeSnapshot>;
        /// Changed attributes clients see.
        fn drain_dirty_syncable_attributes() -> Vec<AttributeSnapshot>;
        /// All equipment clients see, for spawning.
        fn pack_all_equipment() -> Vec<EquipmentSlotItem>;
        /// Changed equipment clients see.
        fn drain_dirty_equipment() -> Vec<EquipmentSlotItem>;
        /// The sheep color and sheared state for loot predicates, when this is a sheep.
        fn sheep_loot_state() -> Option<(DyeColor, bool)>;
        /// Returns the volume used for this entity's sounds.
        fn sound_volume() -> f32;
        /// Runs vanilla `LivingEntity.handleKillingBlow`.
        fn handle_killing_blow();
        /// `LivingEntity.getDefaultDimensions`: the size in `pose` before
        /// the scale attribute applies.
        fn default_dimensions(pose: EntityPose) -> EntityDimensions;
        /// `LivingEntity.pushEntities`: pushes nearby colliding entities away.
        fn push_entities();
        /// `LivingEntity.shouldDropLoot`.
        fn should_drop_loot(world: &World) -> bool;
        /// `LivingEntity.shouldDropExperience`.
        fn should_drop_experience() -> bool;
        /// `LivingEntity.isAlwaysExperienceDropper`.
        fn is_always_experience_dropper() -> bool;
        /// Hook for non-mob custom death loot.
        fn drop_custom_death_loot(source: &DamageSource, killed_by_player: bool);
        /// Checks if the entity is blocking with a shield or similar item.
        fn is_blocking() -> bool;
        /// Returns whether the given item in the given slot allows elytra-style gliding.
        fn can_glide_using(item_stack: &ItemStack, slot: EquipmentSlot) -> bool;
        /// Transforms the rider's raw input into this entity's movement input while ridden.
        fn ridden_input(controller: &Player, self_input: DVec3) -> DVec3;
        /// `LivingEntity.getRiddenSpeed`.
        fn ridden_speed(controller: &Player) -> f32;
        /// Returns the water-movement slowdown multiplier.
        fn water_slow_down() -> f32;
        /// Returns the vertical friction used by `travelInAir`.
        fn air_travel_vertical_friction(horizontal_friction: f32) -> f32;
        /// `LivingEntity.aiStep`, with the movement it performed.
        fn ai_step() -> Option<MoveResult>;
        /// `LivingEntity.travel`, with the movement it performed.
        fn travel(input: DVec3) -> Option<MoveResult>;
        /// `LivingEntity.jumpFromGround`.
        fn jump_from_ground();
        /// `LivingEntity.isImmobile`.
        fn is_immobile() -> bool;
        /// `LivingEntity.canGlide`.
        fn can_glide() -> bool;
        /// `LivingEntity.isInvulnerableTo`.
        fn is_invulnerable_to(world: &World, source: &DamageSource) -> bool;
        /// `LivingEntity.canBeSeenAsEnemy`.
        fn can_be_seen_as_enemy() -> bool;
        /// `LivingEntity.getVoicePitch`.
        fn voice_pitch() -> f32;
        /// `LivingEntity.getEquipSound`, as far as Steel resolves it.
        fn equip_sound(slot: EquipmentSlot, stack: &ItemStack) -> Option<SoundEventRef>;
        /// `LivingEntity.getBaseExperienceReward`.
        fn base_experience_reward() -> i32;
        /// `LivingEntity.canUseSlot`.
        fn can_use_slot(slot: EquipmentSlot) -> bool;
        /// `LivingEntity.canDispenserEquipIntoSlot`.
        fn can_dispenser_equip_into_slot(slot: EquipmentSlot) -> bool;
        /// `LivingEntity.tickRidden`: side effects of a rider steering this
        /// entity.
        fn tick_ridden(controller: &Player, ridden_input: DVec3);
    }
}

/// An entity seen as a vanilla `LivingEntity`. Derefs to the entity.
#[derive(Clone, Copy)]
pub struct Living<'a> {
    entity: &'a (dyn Entity + 'static),
    base: &'a LivingEntityBase,
    hooks: &'a dyn LivingHooks,
}

impl<'a> Living<'a> {
    /// Pairs a concrete living entity with its state.
    #[must_use]
    pub fn new<E: LivingTable>(entity: &'a E) -> Self {
        Self {
            entity,
            base: entity.get::<LivingEntityBase>(),
            hooks: entity,
        }
    }

    /// Returns `entity` as a living entity, if it is one.
    #[must_use]
    pub fn of(entity: &'a (dyn Entity + 'static)) -> Option<Self> {
        let hooks = entity.as_living_hooks()?;
        Some(Self {
            entity,
            base: hooks.layer_component(),
            hooks,
        })
    }

    /// Returns the entity this view belongs to.
    #[must_use]
    pub const fn entity(&self) -> &'a (dyn Entity + 'static) {
        self.entity
    }

    fn with_synced<R>(&self, f: impl FnOnce(&mut LivingEntityData) -> R) -> Option<R> {
        let synced = self.entity.synced_data()?;
        let mut data = synced.lock_data();
        data.living_entity_layer_mut().map(f)
    }
}

impl Deref for Living<'_> {
    type Target = dyn Entity;

    fn deref(&self) -> &Self::Target {
        self.entity
    }
}

/// `LivingEntity`'s [`EntityProps::tick`].
///
/// `LivingEntity.tick` after its `super.tick()`.
pub fn tick<E: Parts>(entity: &E) {
    let living = &entity.living();
    entity::tick(entity);
    living.living_base().decrement_invulnerable_time();

    if living.is_dead_or_dying() {
        living.tick_death();
    }

    living.tick_mob_effects();
    living.detect_equipment_updates();

    if !living.is_removed() {
        let _movement = living.ai_step();
    }

    living.tick_living_state();
}

/// `LivingEntity`'s [`EntityProps::base_tick`].
///
/// Runs the shared per-tick living-entity behavior: rotation/animation
/// snapshots, base entity tick, environmental damage, the hurt timer, and
/// combat memory expiry. Memory must expire before mob effects tick, so a
/// lethal effect tick sees the same kill credit as vanilla.
pub fn base_tick<E: Parts>(entity: &E) {
    let living = &entity.living();
    living.advance_living_rotation_for_base_tick();
    living.living_base().tick_swing_state();
    entity::base_tick(entity);
    living.tick_living_environmental_damage();
    living.living_base().decrement_hurt_time();
    living.living_base().tick_last_hurt_by_player_memory();
    living
        .living_base()
        .tick_living_combat_memory(living.tick_count());
}

/// `LivingEntity`'s [`EntityProps::set_y_body_rot`].
pub fn set_y_body_rot<E: Parts>(living: &E, y_body_rot: f32) {
    entity::set_y_body_rot(living, y_body_rot);
    living.living().set_y_body_rot(y_body_rot);
}

/// `LivingEntity`'s [`EntityProps::stop_riding`].
///
/// `LivingEntity.stopRiding` places the entity beside the vehicle
/// `super` just left.
pub fn stop_riding<E: Parts>(living: &E) {
    let old_vehicle = living.vehicle();
    entity::stop_riding(living);
    if let Some(old_vehicle) = old_vehicle
        && living.vehicle().map(|vehicle| vehicle.id()) != Some(old_vehicle.id())
    {
        living.living().dismount_vehicle(old_vehicle.as_ref());
    }
}

/// `LivingEntity`'s [`EntityProps::ride_tick`].
pub fn ride_tick<E: Parts>(living: &E) {
    entity::ride_tick(living);
    living.reset_fall_distance();
}

/// `LivingEntity`'s [`EntityProps::on_below_world`].
pub fn on_below_world<E: Parts>(living: &E) {
    if let Some(world) = living.level() {
        living.hurt(
            &world,
            &DamageSource::environment(&vanilla_damage_types::OUT_OF_WORLD),
            4.0,
        );
    }
}

/// `LivingEntity`'s [`EntityProps::is_alive`].
pub fn is_alive<E: Parts>(living: &E) -> bool {
    !living.is_removed() && living.living().health() > 0.0
}

/// `LivingEntity`'s [`EntityProps::update_data_before_sync`].
pub fn update_data_before_sync<E: Parts>(living: &E) {
    living.living().sync_mob_effect_data();
}

/// `LivingEntity`'s [`EntityProps::is_pickable`].
pub fn is_pickable<E: Parts>(living: &E) -> bool {
    !living.is_removed()
}

/// `LivingEntity`'s [`EntityProps::is_pushable`].
pub fn is_pushable<E: Parts>(living: &E) -> bool {
    living.is_alive() && !living.is_spectator() && !living.on_climbable()
}

/// `LivingEntity`'s [`EntityProps::default_gravity`].
pub fn default_gravity<E: Parts>(living: &E) -> f64 {
    living.living().attribute_gravity()
}

/// `LivingEntity`'s [`EntityProps::max_up_step`].
pub fn max_up_step<E: Parts>(living: &E) -> f32 {
    living
        .living()
        .attributes()
        .lock()
        .get_value(vanilla_attributes::STEP_HEIGHT)
        .unwrap_or(0.6) as f32
}

/// `LivingEntity`'s [`EntityProps::dimensions_for_pose`].
pub fn dimensions_for_pose<E: Parts>(living: &E, pose: EntityPose) -> EntityDimensions {
    living.living().dimensions(pose)
}

/// `LivingEntity`'s [`EntityProps::is_hard_collision_relevant`].
///
/// Living entities are never hard colliders; a type that overrides
/// `can_be_collided_with` (a shulker) must set this back to `true`.
pub const fn is_hard_collision_relevant<E: Parts>(_: &E) -> bool {
    false
}

/// `LivingEntity`'s [`EntityProps::kill`].
pub fn kill<E: Parts>(living: &E, world: &World) {
    living.hurt(
        world,
        &DamageSource::environment(&vanilla_damage_types::GENERIC_KILL),
        f32::MAX,
    );
}

/// `LivingEntity`'s [`EntityProps::push_entity`].
pub fn push_entity<E: Parts>(living: &E, entity: &dyn Entity) {
    if !living.living().is_sleeping() {
        entity::push_entity(living, entity);
    }
}

/// `LivingEntity`'s [`EntityProps::can_use_portal`].
pub fn can_use_portal<E: Parts>(living: &E, ignore_passenger: bool) -> bool {
    entity::can_use_portal(living, ignore_passenger) && !living.living().is_sleeping()
}

/// `LivingEntity`'s [`EntityProps::relative_portal_position`].
pub fn relative_portal_position<E: Parts>(
    living: &E,
    axis: Axis,
    portal_area: FoundRectangle,
) -> DVec3 {
    reset_forward_direction_of_relative_portal_position(entity::relative_portal_position(
        living,
        axis,
        portal_area,
    ))
}

/// `LivingEntity`'s [`EntityProps::y_head_rot`].
pub fn y_head_rot<E: Parts>(living: &E) -> f32 {
    living.living().y_head_rot()
}

/// `LivingEntity`'s [`EntityProps::set_y_head_rot`].
pub fn set_y_head_rot<E: Parts>(living: &E, y_head_rot: f32) {
    living.living().set_y_head_rot(y_head_rot);
}

/// `LivingEntity`'s [`EntityProps::is_in_wall`].
pub fn is_in_wall<E: Parts>(living: &E) -> bool {
    !living.living().is_sleeping() && entity::is_in_wall(living)
}

/// `LivingEntity`'s [`EntityProps::air_drag`].
pub fn air_drag<E: Parts>(living: &E) -> f32 {
    compute_modified_friction(
        DEFAULT_AIR_DRAG,
        living
            .living()
            .attributes()
            .lock()
            .required_value(vanilla_attributes::AIR_DRAG_MODIFIER) as f32,
    )
}

/// `LivingEntity`'s [`EntityProps::effective_gravity`].
pub fn effective_gravity<E: Parts>(living: &E) -> f64 {
    living.living().effective_gravity()
}

/// `LivingEntity`'s [`EntityProps::with_weapon_item`].
pub fn with_weapon_item<E: Parts>(living: &E, visitor: &mut dyn FnMut(Option<&ItemStack>)) {
    living
        .living()
        .with_equipment_slot(EquipmentSlot::MainHand, &mut |item| visitor(Some(item)));
}

/// `LivingEntity`'s overrides of entity hooks; living entity types build their
/// [`EntityProps`] on this.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        tick: tick::<E>,
        base_tick: base_tick::<E>,
        hurt: hurt::<E>,
        set_y_body_rot: set_y_body_rot::<E>,
        stop_riding: stop_riding::<E>,
        ride_tick: ride_tick::<E>,
        check_fall_damage: check_fall_damage::<E>,
        cause_fall_damage: cause_fall_damage::<E>,
        can_freeze: can_freeze::<E>,
        block_speed_factor: block_speed_factor::<E>,
        on_below_world: on_below_world::<E>,
        is_alive: is_alive::<E>,
        update_data_before_sync: update_data_before_sync::<E>,
        is_pickable: is_pickable::<E>,
        is_pushable: is_pushable::<E>,
        default_gravity: default_gravity::<E>,
        max_up_step: max_up_step::<E>,
        dimensions_for_pose: dimensions_for_pose::<E>,
        is_hard_collision_relevant: is_hard_collision_relevant::<E>,
        kill: kill::<E>,
        push_entity: push_entity::<E>,
        can_use_portal: can_use_portal::<E>,
        relative_portal_position: relative_portal_position::<E>,
        y_head_rot: y_head_rot::<E>,
        set_y_head_rot: set_y_head_rot::<E>,
        is_in_wall: is_in_wall::<E>,
        air_drag: air_drag::<E>,
        effective_gravity: effective_gravity::<E>,
        with_weapon_item: with_weapon_item::<E>,
        ..EntityProps::new()
    }
}

/// A concrete living entity type: its component and its hook table.
pub trait LivingTable: Entity + Has<LivingEntityBase> + Sized {
    /// The `LivingEntity` hooks this type runs, built on its layer's preset
    /// (`LivingProps::new()`, `mob::living_props()`, ...).
    const LIVING: LivingProps<Self>;
}

/// Every living entity type; keeps generic bounds short.
pub trait Parts: LivingTable {
    /// Returns this entity as a living entity.
    fn living(&self) -> Living<'_>;
}

impl<E: LivingTable> Parts for E {
    fn living(&self) -> Living<'_> {
        Living::new(self)
    }
}

impl<'a> Living<'a> {
    /// Returns the shared living runtime state: attributes, cached movement
    /// speed, damage cooldown and death animation counters.
    #[must_use]
    pub const fn living_base(&self) -> &'a LivingEntityBase {
        self.base
    }

    /// Returns vanilla living body/head rotation state.
    #[must_use]
    pub fn living_rotation_state(&self) -> LivingRotationState {
        self.living_base().rotation_state()
    }

    /// Returns the entity's body yaw, used for turning smoothing separate from head yaw.
    #[must_use]
    pub fn y_body_rot(&self) -> f32 {
        self.living_base().y_body_rot()
    }

    /// Sets the entity's body yaw.
    pub fn set_y_body_rot(&self, y_body_rot: f32) {
        self.living_base().set_y_body_rot(y_body_rot);
    }

    /// Returns the entity's head yaw.
    #[must_use]
    pub fn y_head_rot(&self) -> f32 {
        self.living_base().y_head_rot()
    }

    /// Sets the entity's head yaw.
    pub fn set_y_head_rot(&self, y_head_rot: f32) {
        self.living_base().set_y_head_rot(y_head_rot);
    }

    /// Copies current living body/head rotations to vanilla old-rotation state.
    pub fn advance_living_rotation_for_base_tick(&self) {
        self.living_base().advance_rotation_for_base_tick();
    }

    /// Returns vanilla arm-swing animation state.
    #[must_use]
    pub fn living_swing_state(&self) -> LivingSwingState {
        self.living_base().swing_state()
    }

    /// Returns the animation duration adjusted by Haste or Mining Fatigue.
    #[must_use]
    pub fn modified_swing_duration(&self, animation: SwingAnimation) -> i32 {
        let swing_duration = animation.duration;
        if let Some(haste) = self.mob_effect(vanilla_mob_effects::HASTE) {
            swing_duration - (1 + haste.amplifier())
        } else if let Some(mining_fatigue) = self.mob_effect(vanilla_mob_effects::MINING_FATIGUE) {
            swing_duration + (1 + mining_fatigue.amplifier()) * 2
        } else {
            swing_duration
        }
    }

    /// Returns the held item's attack animation.
    #[must_use]
    pub fn attack_animation(&self, hand: InteractionHand) -> SwingAnimation {
        self.held_swing_animation(hand, ATTACK_ANIMATION)
    }

    /// Returns the held item's interaction animation.
    #[must_use]
    pub fn interact_animation(&self, hand: InteractionHand) -> SwingAnimation {
        self.held_swing_animation(hand, INTERACT_ANIMATION)
    }

    /// Reads a swing animation component off the item held in `hand`, falling
    /// back to the vanilla default when the item does not carry one.
    #[must_use]
    pub fn held_swing_animation(
        &self,
        hand: InteractionHand,
        component: DataComponentType<SwingAnimation>,
    ) -> SwingAnimation {
        let slot = match hand {
            InteractionHand::MainHand => EquipmentSlot::MainHand,
            InteractionHand::OffHand => EquipmentSlot::OffHand,
        };
        let mut animation = SwingAnimation::DEFAULT;
        self.with_equipment_slot(slot, &mut |item_stack| {
            animation = item_stack
                .get(component.clone())
                .copied()
                .unwrap_or(SwingAnimation::DEFAULT);
        });
        animation
    }

    /// Sends the animation to tracking players and returns whether a swing started
    #[expect(
        clippy::must_use_candidate,
        reason = "callers that start a swing unconditionally ignore whether one started"
    )]
    pub fn swing(
        &self,
        hand: InteractionHand,
        animation: SwingAnimation,
        update_self: bool,
    ) -> bool {
        if !self
            .living_base()
            .start_swing(hand, animation, self.modified_swing_duration(animation))
        {
            return false;
        }

        let Some(world) = self.level() else {
            return true;
        };

        let packet = CSwingAnimation::new(self.id(), hand, animation);
        let exclude = if update_self { None } else { Some(self.id()) };
        world.broadcast_to_entity_trackers(self.id(), packet.clone(), exclude);
        if update_self && let Some(player) = self.as_player() {
            player.send_packet(packet);
        }

        true
    }

    /// Swings with the held item's attack animation.
    pub fn swing_for_attack(&self, hand: InteractionHand) {
        self.swing(hand, self.attack_animation(hand), false);
    }

    /// Returns a reference to this entity's attribute map.
    #[must_use]
    pub const fn attributes(&self) -> &'a CheckedMutex<AttributeMap> {
        self.living_base().attributes()
    }

    /// Drains dirty mob-effect packet changes for vanilla recipients.
    #[must_use]
    pub fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        self.living_base().drain_dirty_mob_effects()
    }

    /// Saves equipment slots to NBT.
    pub fn save_equipment(&self, nbt: &mut NbtCompound) {
        let mut equipment = NbtCompound::new();
        for slot in EquipmentSlot::ALL {
            self.with_equipment_slot(slot, &mut |item| {
                if !item.is_empty() {
                    equipment.insert(slot.name(), item.to_nbt_tag_ref());
                }
            });
        }
        if !equipment.is_empty() {
            nbt.insert("equipment", NbtTag::Compound(equipment));
        }
    }

    /// Loads equipment slots from NBT.
    pub fn load_equipment(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(equipment_tag) = nbt.compound("equipment") {
            let mut equipment = self.living_base().equipment().lock();
            for slot in EquipmentSlot::ALL {
                let item = equipment_tag
                    .get(slot.name())
                    .and_then(|tag| tag.compound())
                    .and_then(|comp| ItemStack::from_borrowed_compound(&comp))
                    .unwrap_or_else(ItemStack::empty);
                equipment.set(slot, item);
            }
        }
        for slot in EquipmentSlot::ALL {
            self.refresh_equipment_attribute_modifiers(slot);
        }
    }

    /// Appends vanilla-shaped living state used by command NBT predicates.
    pub fn save_command_nbt(&self, nbt: &mut NbtCompound) {
        nbt.insert("Health", self.health());
        nbt.insert(
            "HurtTime",
            NbtTag::Short(self.living_base().hurt_time() as i16),
        );
        nbt.insert(
            "DeathTime",
            NbtTag::Short(self.living_base().death_time() as i16),
        );
        nbt.insert("AbsorptionAmount", self.absorption_amount());
        nbt.insert(
            "current_impulse_context_reset_grace_time",
            self.living_base()
                .current_impulse_context_reset_grace_time(),
        );
        if let Some(impact) = self.living_base().current_impulse_impact_pos() {
            nbt.insert(
                "current_explosion_impact_pos",
                NbtList::Double(vec![impact.x, impact.y, impact.z]),
            );
        }
        nbt.insert("attributes", self.attributes().lock().to_vanilla_nbt());

        let mut effects = self.living_base().active_mob_effects();
        effects.sort_by_key(|effect| effect.effect().try_id().unwrap_or(usize::MAX));
        if !effects.is_empty() {
            nbt.insert(
                "active_effects",
                NbtList::Compound(
                    effects
                        .iter()
                        .map(ActiveMobEffect::to_vanilla_nbt)
                        .collect(),
                ),
            );
        }

        nbt.insert("FallFlying", nbt_bool(self.is_fall_flying()));
        if let Some(pos) = self.sleeping_pos() {
            nbt.insert(
                "sleeping_pos",
                NbtTag::IntArray(vec![pos.x(), pos.y(), pos.z()]),
            );
        }
        if let Some(uuid) = self.last_hurt_by_player_uuid() {
            nbt.insert(
                "last_hurt_by_player",
                NbtTag::IntArray(uuid.to_int_array().to_vec()),
            );
            nbt.insert(
                "last_hurt_by_player_memory_time",
                self.last_hurt_by_player_memory_time(),
            );
        }
        if let Some(entity) = self.last_hurt_by_mob() {
            nbt.insert(
                "last_hurt_by_mob",
                NbtTag::IntArray(entity.uuid().to_int_array().to_vec()),
            );
            nbt.insert(
                "ticks_since_last_hurt_by_mob",
                self.tick_count()
                    .wrapping_sub(self.last_hurt_by_mob_timestamp()),
            );
        }

        if self.as_mob().is_none() {
            self.save_equipment(nbt);
        }
    }

    /// Gets the current health of the entity.
    #[must_use]
    pub fn health(&self) -> f32 {
        self.with_synced(|data| *data.health.get()).unwrap_or(0.0)
    }

    /// Sets the health of the entity, clamped between 0 and max health.
    pub fn set_health(&self, health: f32) {
        let health = health.clamp(0.0, self.max_health());
        self.with_synced(|data| data.health.set(health));
    }

    /// Gets the maximum health from the attribute system.
    #[must_use]
    pub fn max_health(&self) -> f32 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::MAX_HEALTH) as f32
    }

    /// Returns ticks this entity has gone without taking a noteworthy action.
    #[must_use]
    pub fn no_action_time(&self) -> i32 {
        self.living_base().no_action_time()
    }

    /// Sets ticks this entity has gone without taking a noteworthy action.
    pub fn set_no_action_time(&self, no_action_time: i32) {
        self.living_base().set_no_action_time(no_action_time);
    }

    /// Increments the no-action-time counter by one tick.
    pub fn increment_no_action_time(&self) {
        self.living_base().increment_no_action_time();
    }

    /// Heals the entity by the specified amount.
    pub fn heal(&self, amount: f32) {
        let current_health = self.health();
        if current_health > 0.0 {
            self.set_health(current_health + amount);
        }
    }

    /// Returns true if the entity is dead or dying (health <= 0).
    #[must_use]
    pub fn is_dead_or_dying(&self) -> bool {
        self.health() <= 0.0
    }

    /// Returns whether this entity is currently a baby.
    #[must_use]
    pub fn is_baby(&self) -> bool {
        Ageable::of(self.as_entity_event_source()).is_some_and(|ageable| ageable.is_baby())
    }

    /// Plays `sound` at this entity's current volume and pitch, if any.
    pub fn make_sound(&self, sound: Option<SoundEventRef>) {
        if let Some(sound) = sound {
            self.play_sound(sound, self.sound_volume(), self.voice_pitch());
        }
    }

    /// Plays this entity's hurt sound and resets its ambient-sound timer if it's a mob.
    pub fn play_hurt_sound(&self, source: &DamageSource) {
        if let Some(mob) = self.as_mob() {
            mob.reset_ambient_sound_time();
        }
        self.make_sound(self.hurt_sound(source));
    }

    /// Plays vanilla's death sound for this living entity.
    pub fn play_death_sound(&self) {
        self.make_sound(self.death_sound());
    }

    /// `LivingEntity.getDimensions`: the default size for `pose`, scaled by
    /// the scale attribute, or a small fixed box while sleeping.
    #[must_use]
    pub fn dimensions(&self, pose: EntityPose) -> EntityDimensions {
        if pose == EntityPose::Sleeping {
            return SLEEPING_DIMENSIONS;
        }
        let dimensions = self.default_dimensions(pose);
        if self.entity_type().fixed {
            dimensions
        } else {
            dimensions.scale(self.scale())
        }
    }

    /// Returns the render scale from age: 0.5 for babies, 1.0 for adults.
    #[must_use]
    pub fn age_scale(&self) -> f32 {
        if self.is_baby() { 0.5 } else { 1.0 }
    }

    /// Returns this entity's overall render scale, from the scale attribute.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::SCALE)
            .unwrap_or(1.0) as f32
    }

    /// Returns true if the entity is alive (health > 0).
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.is_dead_or_dying()
    }

    /// Returns the fraction of armor slots that currently have equipment,
    /// used for armor-tint rendering.
    #[must_use]
    pub fn armor_cover_percentage(&self) -> f32 {
        let mut covered_slots = 0;
        for slot in EquipmentSlot::ARMOR_SLOTS {
            self.with_equipment_slot(slot, &mut |item_stack| {
                if !item_stack.is_empty() {
                    covered_slots += 1;
                }
            });
        }

        covered_slots as f32 / EquipmentSlot::ARMOR_SLOTS.len() as f32
    }

    /// Returns how visible this entity is, from 0.0 to 1.0: reduced while
    /// sneaking, invisible, or disguised with a matching mob head.
    #[must_use]
    pub fn visibility_percent(&self, targeting_entity: Option<&dyn Entity>) -> f64 {
        let mut visibility_percent = 1.0;
        if self.is_discrete() {
            visibility_percent *= 0.8;
        }

        if self.is_invisible() {
            visibility_percent *= 0.7 * f64::from(self.armor_cover_percentage().max(0.1));
        }

        if self.disguise_head_matches_targeting_entity(targeting_entity) {
            visibility_percent *= 0.5;
        }

        visibility_percent
    }

    /// Returns whether the equipped head item reduces visibility to `targeting_entity`.
    #[must_use]
    pub fn disguise_head_matches_targeting_entity(
        &self,
        targeting_entity: Option<&dyn Entity>,
    ) -> bool {
        let Some(targeting_entity) = targeting_entity else {
            return false;
        };

        let mut matches_target = false;
        self.with_equipment_slot(EquipmentSlot::Head, &mut |item_stack| {
            let target_type = targeting_entity.entity_type();
            matches_target = target_type == &vanilla_entities::SKELETON
                && item_stack.is(&vanilla_items::SKELETON_SKULL)
                || target_type == &vanilla_entities::ZOMBIE
                    && item_stack.is(&vanilla_items::ZOMBIE_HEAD)
                || target_type == &vanilla_entities::PIGLIN
                    && item_stack.is(&vanilla_items::PIGLIN_HEAD)
                || target_type == &vanilla_entities::PIGLIN_BRUTE
                    && item_stack.is(&vanilla_items::PIGLIN_HEAD)
                || target_type == &vanilla_entities::CREEPER
                    && item_stack.is(&vanilla_items::CREEPER_HEAD);
        });
        matches_target
    }

    /// Returns whether this entity is currently visible to anything: alive and not a spectator.
    #[must_use]
    pub fn can_be_seen_by_anyone(&self) -> bool {
        !self.is_spectator() && EntityHooks::is_alive(self.entity())
    }

    /// Ticks living-entity counters after movement.
    pub fn tick_living_state(&self) {
        if let Some(mob) = self.as_mob() {
            mob.tick_body_rotation_control();
        }
        self.living_base()
            .tick_fall_flying_state(self.is_fall_flying());
        self.refresh_dirty_attributes();
        self.living_base().tick_post_impulse_grace_time();
    }

    /// Drains dirty attributes and applies server-side effects.
    pub fn refresh_dirty_attributes(&self) {
        let dirty = self.attributes().lock().drain_dirty_updates();
        for attr in dirty {
            if attr.key == vanilla_attributes::MAX_HEALTH.key {
                let max = self.max_health();
                if self.health() > max {
                    self.set_health(max);
                }
            } else if attr.key == vanilla_attributes::MAX_ABSORPTION.key {
                let max = self
                    .attributes()
                    .lock()
                    .get_value(vanilla_attributes::MAX_ABSORPTION)
                    .unwrap_or(0.0) as f32;
                if self.absorption_amount() > max {
                    self.set_absorption_amount(max);
                }
            } else if attr.key == vanilla_attributes::SCALE.key {
                self.refresh_dimensions();
            }
            // TODO: WAYPOINT_TRANSMIT_RANGE → waypoint manager
        }
    }
}

/// `LivingEntity`'s [`LivingProps::luck`].
///
/// Vanilla `LivingEntity.getLuck` - non-players contribute no luck to loot.
#[must_use]
pub const fn luck<E: Parts>(_entity: &E) -> f32 {
    0.0
}

/// `LivingEntity`'s [`LivingProps::pack_syncable_attributes`].
///
/// Packs syncable attributes for initial spawn pairing.
///
/// Mirrors vanilla `ServerEntity.sendPairingData`, which sends all syncable
/// living attributes after the add-entity and metadata packets.
#[must_use]
pub fn pack_syncable_attributes<E: Parts>(entity: &E) -> Vec<AttributeSnapshot> {
    let living = &entity.living();
    living.attributes().lock().syncable_snapshots()
}

/// `LivingEntity`'s [`LivingProps::drain_dirty_syncable_attributes`].
///
/// Drains syncable dirty attributes for per-tick tracking updates.
///
/// Mirrors vanilla `ServerEntity.sendDirtyEntityData`, which sends dirty
/// living attributes after dirty entity data.
#[must_use]
pub fn drain_dirty_syncable_attributes<E: Parts>(entity: &E) -> Vec<AttributeSnapshot> {
    let living = &entity.living();
    living.attributes().lock().drain_dirty_sync()
}

/// `LivingEntity`'s [`LivingProps::pack_all_equipment`].
///
/// Packs non-empty equipment slots for initial spawn pairing.
#[must_use]
pub fn pack_all_equipment<E: Parts>(entity: &E) -> Vec<EquipmentSlotItem> {
    let living = &entity.living();
    living.pack_living_equipment()
}

/// `LivingEntity`'s [`LivingProps::drain_dirty_equipment`].
///
/// Drains equipment slots that changed since the last tracker sync.
#[must_use]
pub fn drain_dirty_equipment<E: Parts>(entity: &E) -> Vec<EquipmentSlotItem> {
    let living = &entity.living();
    living.drain_dirty_living_equipment()
}

/// `LivingEntity`'s [`LivingProps::sheep_loot_state`].
///
/// Returns the vanilla sheep loot predicate state (`minecraft:components.sheep/color`
/// together with `minecraft:type_specific/sheep.sheared`), when this entity is a sheep.
///
/// Mirrors `Sheep.get(DataComponents.SHEEP_COLOR)` + `Sheep.isSheared()` for the
/// entity loot context.
#[must_use]
pub const fn sheep_loot_state<E: Parts>(_entity: &E) -> Option<(DyeColor, bool)> {
    None
}

/// `LivingEntity`'s [`LivingProps::chicken_loot_variant`].
///
/// Returns this entity's `minecraft:components.chicken/variant` key for the
/// entity loot context, when it is a chicken.
///
/// Mirrors `Chicken.get(DataComponents.CHICKEN_VARIANT)` for the loot predicate.
#[must_use]
pub const fn chicken_loot_variant<E: Parts>(_entity: &E) -> Option<&'static Identifier> {
    None
}

/// `LivingEntity`'s [`LivingProps::sound_volume`].
///
/// Returns the volume used for this entity's sounds.
#[must_use]
pub const fn sound_volume<E: Parts>(_entity: &E) -> f32 {
    1.0
}

/// `LivingEntity`'s [`LivingProps::voice_pitch`].
///
/// Returns the pitch used for this entity's sounds; babies get a randomized higher pitch.
#[must_use]
pub fn voice_pitch<E: Parts>(entity: &E) -> f32 {
    let living = &entity.living();
    if living.is_baby() {
        (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.5
    } else {
        (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.0
    }
}

/// `LivingEntity`'s [`LivingProps::base_experience_reward`]: none.
pub const fn base_experience_reward<E: Parts>(_entity: &E) -> i32 {
    0
}

/// `LivingEntity`'s [`LivingProps::can_use_slot`]: any slot.
#[must_use]
pub const fn can_use_slot<E: Parts>(_entity: &E, _slot: EquipmentSlot) -> bool {
    true
}

/// `LivingEntity`'s [`LivingProps::can_dispenser_equip_into_slot`]: any slot.
pub const fn can_dispenser_equip_into_slot<E: Parts>(_entity: &E, _slot: EquipmentSlot) -> bool {
    true
}

/// `LivingEntity`'s [`LivingProps::tick_ridden`]: vanilla's does nothing.
pub const fn tick_ridden<E: Parts>(_entity: &E, _controller: &Player, _ridden_input: DVec3) {}

/// `LivingEntity`'s [`LivingProps::hurt_sound`].
///
/// Returns the sound played when this entity is hurt.
#[must_use]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the hook lets entity types return no sound"
)]
pub fn hurt_sound<E: Parts>(_entity: &E, _source: &DamageSource) -> Option<SoundEventRef> {
    Some(&sound_events::ENTITY_GENERIC_HURT)
}

/// `LivingEntity`'s [`LivingProps::death_sound`].
///
/// Returns the sound played when this entity dies.
#[must_use]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the hook lets entity types return no sound"
)]
pub fn death_sound<E: Parts>(_entity: &E) -> Option<SoundEventRef> {
    Some(&sound_events::ENTITY_GENERIC_DEATH)
}

/// `LivingEntity`'s [`LivingProps::default_dimensions`]: the type's size, or
/// its extracted baby size for babies. The extracted size covers both
/// vanilla's age scaling and types' own `BABY_DIMENSIONS`.
#[must_use]
pub fn default_dimensions<E: Parts>(entity: &E, _pose: EntityPose) -> EntityDimensions {
    let living = &entity.living();
    let entity_type = living.entity_type();
    if living.is_baby()
        && let Some(baby_dimensions) = entity_type.baby_dimensions
    {
        return baby_dimensions;
    }
    if entity_type.fixed {
        entity_type.dimensions
    } else {
        entity_type.dimensions.scale(living.age_scale())
    }
}

/// `LivingEntity`'s [`LivingProps::can_be_seen_as_enemy`].
///
/// Returns whether this entity can be targeted as an enemy: visible and not invulnerable.
#[must_use]
pub fn can_be_seen_as_enemy<E: Parts>(entity: &E) -> bool {
    let living = &entity.living();
    !living.is_invulnerable() && living.can_be_seen_by_anyone()
}
