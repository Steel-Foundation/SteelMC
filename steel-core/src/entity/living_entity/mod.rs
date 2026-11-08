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
mod effects;
mod environment;
mod equipment;
mod movement;
mod sleeping;
mod travel;

const BED_SLEEP_POSITION_OFFSET: f64 = 0.125;
/// `LivingEntity.SLEEPING_DIMENSIONS`.
const SLEEPING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 0.2);

entity_props! {
    /// Per-type properties of a vanilla `LivingEntity`.
    pub struct LivingProps<E>: LivingHooks for Living
    where E: Parts => living {
        /// Current absorption hearts.
        fn absorption_amount() -> f32 = absorption_amount_default;
        /// Sets the absorption hearts.
        fn set_absorption_amount(amount: f32) = set_absorption_amount_default;
        /// Reacts to an equipped item breaking.
        fn on_equipped_item_broken(item: ItemRef, slot: EquipmentSlot) = on_equipped_item_broken_default;
        /// `LivingEntity.canBeSeenAsEnemy`.
        fn can_be_seen_as_enemy() -> bool = can_be_seen_as_enemy_default;
        /// `LivingEntity.isAffectedByFluids`.
        fn is_affected_by_fluids() -> bool = is_affected_by_fluids_default;
        /// `LivingEntity.aiStep`.
        fn ai_step(entity: &SharedEntity) -> Option<MoveResult> = ai_step_default;
        /// `LivingEntity.indicateDamage`.
        fn indicate_damage(xd: f64, zd: f64) = indicate_damage_default;
        /// `LivingEntity.tick`.
        fn tick_living_entity(entity: &SharedEntity) = tick_living_entity_default;
        /// `LivingEntity.isUsingItem`.
        fn is_using_item() -> bool = is_using_item_default;
        /// `LivingEntity.getLuck`.
        fn luck() -> f32 = luck_default;
        /// `LivingEntity.isInvulnerableTo`.
        fn is_invulnerable_to(world: &World, source: &DamageSource) -> bool = is_invulnerable_to_default;
        /// `LivingEntity.hurtArmor`.
        fn hurt_armor(source: &DamageSource, damage: f32) = hurt_armor_default;
        /// `LivingEntity.hurtHelmet`, for damage that hits a worn helmet.
        fn hurt_helmet(source: &DamageSource, damage: f32) = hurt_helmet_default;
        /// `LivingEntity.getVoicePitch`.
        fn voice_pitch() -> f32 = voice_pitch_default;
        /// `LivingEntity.doesEmitEquipEvent`.
        fn does_emit_equip_event(slot: EquipmentSlot) -> bool = does_emit_equip_event_default;
        /// `LivingEntity.actuallyHurt`.
        fn actually_hurt(world: &World, source: &DamageSource, amount: f32) = actually_hurt_default;
        /// The chunk whose viewers receive this entity's hurt animation.
        fn hurt_broadcast_chunk() -> ChunkPos = hurt_broadcast_chunk_default;
        /// `LivingEntity.die`.
        fn die(source: &DamageSource) = die_default;
        /// Ticks equipment change detection.
        fn tick_equipment() = tick_equipment_default;
        /// Visits the item in `slot`.
        fn with_equipment_slot(slot: EquipmentSlot, visitor: &mut dyn FnMut(&ItemStack)) = with_equipment_slot_default;
        /// Visits the item in `slot` mutably.
        fn with_equipment_slot_mut(slot: EquipmentSlot, visitor: &mut dyn FnMut(&mut ItemStack)) = with_equipment_slot_mut_default;
        /// Equips `player`'s held item onto this entity.
        fn interact_living_entity_with_equippable(player: &Player, hand: InteractionHand) -> InteractionResult = interact_living_entity_with_equippable_default;
        /// `LivingEntity.hasInfiniteMaterials`.
        fn has_infinite_materials() -> bool = has_infinite_materials_default;
        /// `LivingEntity.handleExtraItemsCreatedOnUse`.
        fn handle_extra_items_created_on_use(extra: ItemStack) = handle_extra_items_created_on_use_default;
        /// `LivingEntity.canGlide`.
        fn can_glide() -> bool = can_glide_default;
        /// `LivingEntity.isImmobile`.
        fn is_immobile() -> bool = is_immobile_default;
        /// `LivingEntity.stopSleeping`.
        fn stop_sleeping() = stop_sleeping_default;
        /// `LivingEntity.jumpFromGround`.
        fn jump_from_ground() = jump_from_ground_default;
        /// `LivingEntity.travel`.
        fn travel(entity: &SharedEntity, input: DVec3) -> Option<MoveResult> = travel_default;
        /// `LivingEntity.getFlyingSpeed`.
        fn flying_speed() -> f32 = flying_speed_default;
        /// `LivingEntity.getHurtSound`.
        fn hurt_sound(source: &DamageSource) -> Option<SoundEventRef> = hurt_sound_default;
        /// `LivingEntity.getDeathSound`.
        fn death_sound() -> Option<SoundEventRef> = death_sound_default;
        /// The chicken variant key for loot predicates, when this is a chicken.
        fn chicken_loot_variant() -> Option<&'static Identifier> = chicken_loot_variant_default;
        /// `LivingEntity.getBaseExperienceReward`.
        fn base_experience_reward() -> i32 = base_experience_reward_default;
        /// `LivingEntity.serverAiStep`.
        fn server_ai_step(entity: &SharedEntity) = server_ai_step_default;
        /// `LivingEntity.canBeAffected`.
        fn can_be_affected(effect: &MobEffectInstance) -> bool = can_be_affected_default;
        /// `LivingEntity.canStandOnFluid`.
        fn can_stand_on_fluid(fluid_state: FluidState) -> bool = can_stand_on_fluid_default;
        /// Whether Frost Walker may act under this entity.
        fn is_on_non_air_block_for_frost() -> bool = is_on_non_air_block_for_frost_default;
        /// All attributes clients see, for spawning.
        fn pack_syncable_attributes() -> Vec<AttributeSnapshot> = pack_syncable_attributes_default;
        /// Changed attributes clients see.
        fn drain_dirty_syncable_attributes() -> Vec<AttributeSnapshot> = drain_dirty_syncable_attributes_default;
        /// All equipment clients see, for spawning.
        fn pack_all_equipment() -> Vec<EquipmentSlotItem> = pack_all_equipment_default;
        /// Changed equipment clients see.
        fn drain_dirty_equipment() -> Vec<EquipmentSlotItem> = drain_dirty_equipment_default;
        /// The sheep color and sheared state for loot predicates, when this is a sheep.
        fn sheep_loot_state() -> Option<(DyeColor, bool)> = sheep_loot_state_default;
        /// Returns the volume used for this entity's sounds.
        fn sound_volume() -> f32 = sound_volume_default;
        /// Runs vanilla `LivingEntity.handleKillingBlow`.
        fn handle_killing_blow() = handle_killing_blow_default;
        /// `LivingEntity.getDefaultDimensions`: the size in `pose` before
        /// the scale attribute applies.
        fn default_dimensions(pose: EntityPose) -> EntityDimensions = default_dimensions_default;
        /// `LivingEntity.pushEntities`: pushes nearby colliding entities away.
        fn push_entities() = push_entities_default;
        /// `LivingEntity.shouldDropLoot`.
        fn should_drop_loot(world: &World) -> bool = should_drop_loot_default;
        /// `LivingEntity.shouldDropExperience`.
        fn should_drop_experience() -> bool = should_drop_experience_default;
        /// `LivingEntity.isAlwaysExperienceDropper`.
        fn is_always_experience_dropper() -> bool = is_always_experience_dropper_default;
        /// Hook for non-mob custom death loot.
        fn drop_custom_death_loot(source: &DamageSource, killed_by_player: bool) = drop_custom_death_loot_default;
        /// Checks if the entity is blocking with a shield or similar item.
        fn is_blocking() -> bool = is_blocking_default;
        /// Returns whether vanilla allows this entity to use `slot`.
        fn can_use_slot(slot: EquipmentSlot) -> bool = can_use_slot_default;
        /// Returns the equip sound Steel can currently resolve for this entity.
        fn equip_sound(slot: EquipmentSlot, stack: &ItemStack) -> Option<SoundEventRef> = equip_sound_default;
        /// Returns whether the given item in the given slot allows elytra-style gliding.
        fn can_glide_using(item_stack: &ItemStack, slot: EquipmentSlot) -> bool = can_glide_using_default;
        /// Ticks side effects of a rider controlling this entity, such as steering-based input.
        fn tick_ridden(controller: &Player, ridden_input: DVec3) = tick_ridden_default;
        /// Transforms the rider's raw input into this entity's movement input while ridden.
        fn ridden_input(controller: &Player, self_input: DVec3) -> DVec3 = ridden_input_default;
        /// `LivingEntity.getRiddenSpeed`.
        fn ridden_speed(controller: &Player) -> f32 = ridden_speed_default;
        /// `LivingEntity.canDispenserEquipIntoSlot`.
        fn can_dispenser_equip_into_slot(slot: EquipmentSlot) -> bool = can_dispenser_equip_into_slot_default;
        /// Returns the water-movement slowdown multiplier.
        fn water_slow_down() -> f32 = water_slow_down_default;
        /// Returns the vertical friction used by `travelInAir`.
        fn air_travel_vertical_friction(horizontal_friction: f32) -> f32 = air_travel_vertical_friction_default;
    }
}

/// An entity seen as a vanilla `LivingEntity`. Derefs to the entity.
#[derive(Clone, Copy)]
pub struct Living<'a> {
    entity: &'a (dyn Entity + 'static),
    base: &'a LivingEntityBase,
}

impl<'a> Living<'a> {
    /// Pairs a concrete living entity with its state.
    #[must_use]
    pub fn new<E: Entity + Has<LivingEntityBase>>(entity: &'a E) -> Self {
        Self {
            entity,
            base: entity.get::<LivingEntityBase>(),
        }
    }

    /// Returns `entity` as a living entity, if it is one.
    #[must_use]
    pub fn of(entity: &'a (dyn Entity + 'static)) -> Option<Self> {
        Some(Self {
            entity,
            base: entity.query::<LivingEntityBase>()?,
        })
    }

    /// Returns the entity this view belongs to.
    #[must_use]
    pub const fn entity(&self) -> &'a (dyn Entity + 'static) {
        self.entity
    }

    fn hooks(&self) -> &'static dyn LivingHooks {
        self.base.hooks()
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

/// `LivingEntity`'s overrides of entity hooks; living entity types build their
/// [`EntityProps`] on this.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        tick: |living, entity| living.living().tick_living_entity(entity),
        ride_tick: |living, entity| {
            Arc::clone(entity).ride_tick_default();
            living.reset_fall_distance();
        },
        base_tick: |living| living.living().base_tick_living_entity(),
        hurt: |living, world, source, amount| living.living().hurt_server(world, source, amount),
        cause_fall_damage: |living, _, fall_distance, damage_modifier, source| {
            living
                .living()
                .cause_living_fall_damage(fall_distance, damage_modifier, source)
        },
        is_alive: |living| !living.is_removed() && living.living().health() > 0.0,
        update_data_before_sync: |living| living.living().sync_mob_effect_data(),
        is_pickable: |living| !living.is_removed(),
        is_pushable: |living| living.is_alive() && !living.is_spectator() && !living.on_climbable(),
        can_freeze: |living| living.living().default_living_can_freeze(),
        default_gravity: |living| living.living().attribute_gravity(),
        max_up_step: |living| {
            living
                .living()
                .attributes()
                .lock()
                .get_value(vanilla_attributes::STEP_HEIGHT)
                .unwrap_or(0.6) as f32
        },
        dimensions_for_pose: |living, pose| living.living().dimensions(pose),
        // Living entities are never hard colliders; a type that overrides
        // `can_be_collided_with` (a shulker) must set this back to `true`.
        is_hard_collision_relevant: |_| false,
        check_fall_damage: |living, entity, vertical_movement, on_ground, on_state, pos, world| {
            living.living().check_living_fall_damage(
                entity,
                vertical_movement,
                on_ground,
                on_state,
                pos,
                world,
            );
        },
        kill: |living, world| {
            living.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::GENERIC_KILL),
                f32::MAX,
            );
        },
        push_entity: |living, entity| {
            if !living.living().is_sleeping() {
                living.push_entity_default(entity);
            }
        },
        can_use_portal: |living, ignore_passenger| {
            living.can_use_portal_default(ignore_passenger) && !living.living().is_sleeping()
        },
        relative_portal_position: |living, axis, portal_area| {
            reset_forward_direction_of_relative_portal_position(
                living.relative_portal_position_default(axis, portal_area),
            )
        },
        y_head_rot: |living| living.living().y_head_rot(),
        set_y_head_rot: |living, y_head_rot| living.living().set_y_head_rot(y_head_rot),
        set_y_body_rot: |living, y_body_rot| living.living().set_y_body_rot(y_body_rot),
        is_in_wall: |living| !living.living().is_sleeping() && living.is_in_wall_default(),
        block_speed_factor: |living| living.living().living_block_speed_factor(),
        air_drag: |living| {
            compute_modified_friction(
                DEFAULT_AIR_DRAG,
                living
                    .living()
                    .attributes()
                    .lock()
                    .required_value(vanilla_attributes::AIR_DRAG_MODIFIER) as f32,
            )
        },
        effective_gravity: |living| living.living().effective_gravity(),
        with_weapon_item: |living, visitor| {
            living
                .living()
                .with_equipment_slot(EquipmentSlot::MainHand, &mut |item| visitor(Some(item)));
        },
        ..EntityProps::new()
    }
}

/// Components every living entity has; keeps generic bounds short.
pub trait Parts: Entity + Has<LivingEntityBase> {
    /// Returns this entity as a living entity.
    fn living(&self) -> Living<'_>;
}

impl<E: Entity + Has<LivingEntityBase>> Parts for E {
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

    /// Runs the shared per-tick living-entity behavior: rotation/animation
    /// snapshots, base entity tick, environmental damage, the hurt timer, and
    /// combat memory expiry. Memory must expire before mob effects tick, so a
    /// lethal effect tick sees the same kill credit as vanilla.
    pub fn base_tick_living_entity(&self) {
        self.advance_living_rotation_for_base_tick();
        self.living_base().tick_swing_state();
        self.entity_base_tick();
        self.tick_living_environmental_damage();
        self.living_base().decrement_hurt_time();
        self.living_base().tick_last_hurt_by_player_memory();
        self.living_base()
            .tick_living_combat_memory(self.tick_count());
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

    /// Vanilla `LivingEntity.getLuck` - non-players contribute no luck to loot.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn luck_default(&self) -> f32 {
        0.0
    }

    /// Packs syncable attributes for initial spawn pairing.
    ///
    /// Mirrors vanilla `ServerEntity.sendPairingData`, which sends all syncable
    /// living attributes after the add-entity and metadata packets.
    #[must_use]
    pub fn pack_syncable_attributes_default(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().syncable_snapshots()
    }

    /// Drains syncable dirty attributes for per-tick tracking updates.
    ///
    /// Mirrors vanilla `ServerEntity.sendDirtyEntityData`, which sends dirty
    /// living attributes after dirty entity data.
    #[must_use]
    pub fn drain_dirty_syncable_attributes_default(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().drain_dirty_sync()
    }

    /// Drains dirty mob-effect packet changes for vanilla recipients.
    #[must_use]
    pub fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        self.living_base().drain_dirty_mob_effects()
    }

    /// Packs non-empty equipment slots for initial spawn pairing.
    #[must_use]
    pub fn pack_all_equipment_default(&self) -> Vec<EquipmentSlotItem> {
        self.pack_living_equipment()
    }

    /// Drains equipment slots that changed since the last tracker sync.
    #[must_use]
    pub fn drain_dirty_equipment_default(&self) -> Vec<EquipmentSlotItem> {
        self.drain_dirty_living_equipment()
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

    /// Returns the vanilla sheep loot predicate state (`minecraft:components.sheep/color`
    /// together with `minecraft:type_specific/sheep.sheared`), when this entity is a sheep.
    ///
    /// Mirrors `Sheep.get(DataComponents.SHEEP_COLOR)` + `Sheep.isSheared()` for the
    /// entity loot context.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn sheep_loot_state_default(&self) -> Option<(DyeColor, bool)> {
        None
    }

    /// Returns this entity's `minecraft:components.chicken/variant` key for the
    /// entity loot context, when it is a chicken.
    ///
    /// Mirrors `Chicken.get(DataComponents.CHICKEN_VARIANT)` for the loot predicate.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn chicken_loot_variant_default(&self) -> Option<&'static Identifier> {
        None
    }

    /// Returns the volume used for this entity's sounds.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn sound_volume_default(&self) -> f32 {
        1.0
    }

    /// Returns the pitch used for this entity's sounds; babies get a randomized higher pitch.
    #[must_use]
    pub fn voice_pitch_default(&self) -> f32 {
        if self.is_baby() {
            (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.5
        } else {
            (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.0
        }
    }

    /// Returns the sound played when this entity is hurt.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the hook lets entity types return no sound"
    )]
    pub fn hurt_sound_default(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_GENERIC_HURT)
    }

    /// Returns the sound played when this entity dies.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the hook lets entity types return no sound"
    )]
    pub fn death_sound_default(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_GENERIC_DEATH)
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

    /// Default for [`LivingProps::default_dimensions`]: the type's size, or
    /// its extracted baby size for babies. The extracted size covers both
    /// vanilla's age scaling and types' own `BABY_DIMENSIONS`.
    #[must_use]
    pub fn default_dimensions_default(&self, _pose: EntityPose) -> EntityDimensions {
        let entity_type = self.entity_type();
        if self.is_baby()
            && let Some(baby_dimensions) = entity_type.baby_dimensions
        {
            return baby_dimensions;
        }
        if entity_type.fixed {
            entity_type.dimensions
        } else {
            entity_type.dimensions.scale(self.age_scale())
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
        !self.is_spectator() && Entity::is_alive(self.entity())
    }

    /// Returns whether this entity can be targeted as an enemy: visible and not invulnerable.
    #[must_use]
    pub fn can_be_seen_as_enemy_default(&self) -> bool {
        !self.is_invulnerable() && self.can_be_seen_by_anyone()
    }

    /// Runs vanilla `LivingEntity.tick`.
    ///
    /// The default `Entity::tick` dispatches living entities here.
    pub fn tick_living_entity_default(&self, entity: &SharedEntity) {
        self.tick_default();
        self.living_base().decrement_invulnerable_time();

        if self.is_dead_or_dying() {
            self.tick_death();
        }

        self.tick_mob_effects();
        self.detect_equipment_updates();

        if !self.is_removed() {
            let _movement = self.ai_step(entity);
        }

        self.tick_living_state();
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
