use super::*;

/// A trait for living entities that can take damage, heal, and die.
///
/// This trait provides the core functionality for entities that have health,
/// can be damaged, and can die. It's based on Minecraft's `LivingEntity` class.
///
/// **Note:** All methods take `&self` (not `&mut self`) because living entities
/// are shared via `Arc` and use interior mutability (`SyncMutex`, etc.).
pub trait LivingEntity: Entity {
    /// Returns a reference to the shared [`LivingEntityBase`] that holds
    /// living runtime state such as attributes, cached movement speed,
    /// damage cooldown, and death animation counters.
    fn living_base(&self) -> &LivingEntityBase;

    /// Returns vanilla living body/head rotation state.
    fn living_rotation_state(&self) -> LivingRotationState {
        self.living_base().rotation_state()
    }

    /// Returns vanilla `LivingEntity.yBodyRot`.
    fn y_body_rot(&self) -> f32 {
        self.living_base().y_body_rot()
    }

    /// Sets vanilla `LivingEntity.yBodyRot`.
    fn set_y_body_rot(&self, y_body_rot: f32) {
        self.living_base().set_y_body_rot(y_body_rot);
    }

    /// Returns vanilla `LivingEntity.yHeadRot`.
    fn y_head_rot(&self) -> f32 {
        self.living_base().y_head_rot()
    }

    /// Sets vanilla `LivingEntity.yHeadRot`.
    fn set_y_head_rot(&self, y_head_rot: f32) {
        self.living_base().set_y_head_rot(y_head_rot);
    }

    /// Copies current living body/head rotations to vanilla old-rotation state.
    fn advance_living_rotation_for_base_tick(&self) {
        self.living_base().advance_rotation_for_base_tick();
    }

    /// Copies current attack animation to vanilla old attack-animation state.
    fn advance_attack_animation_for_base_tick(&self) {
        self.living_base().advance_attack_animation_for_base_tick();
    }

    /// Runs vanilla `LivingEntity.baseTick`.
    fn base_tick_living_entity(&self) {
        self.advance_living_rotation_for_base_tick();
        self.advance_attack_animation_for_base_tick();
        self.entity_base_tick();
        self.tick_living_environmental_damage();
    }

    /// Returns vanilla arm-swing animation state.
    fn living_swing_state(&self) -> LivingSwingState {
        self.living_base().swing_state()
    }

    /// Returns vanilla `LivingEntity.getCurrentSwingDuration`.
    fn current_swing_duration(&self) -> i32 {
        let hand = self
            .living_swing_state()
            .swinging_arm()
            .unwrap_or(InteractionHand::MainHand);
        let slot = match hand {
            InteractionHand::MainHand => EquipmentSlot::MainHand,
            InteractionHand::OffHand => EquipmentSlot::OffHand,
        };
        let mut swing_duration = SwingAnimation::DEFAULT.duration;
        self.with_equipment_slot(slot, &mut |item_stack| {
            swing_duration = item_stack
                .get(SWING_ANIMATION)
                .copied()
                .unwrap_or(SwingAnimation::DEFAULT)
                .duration;
        });
        if let Some(haste) = self.mob_effect(vanilla_mob_effects::HASTE) {
            swing_duration - (1 + haste.amplifier())
        } else if let Some(mining_fatigue) = self.mob_effect(vanilla_mob_effects::MINING_FATIGUE) {
            swing_duration + (1 + mining_fatigue.amplifier()) * 2
        } else {
            swing_duration
        }
    }

    /// Runs vanilla `LivingEntity.swing`.
    fn swing(&self, hand: InteractionHand, update_self: bool) {
        if !self
            .living_base()
            .start_swing(hand, self.current_swing_duration())
        {
            return;
        }

        let Some(world) = self.level() else {
            return;
        };
        let action = match hand {
            InteractionHand::MainHand => AnimateAction::SwingMainHand,
            InteractionHand::OffHand => AnimateAction::SwingOffHand,
        };
        let packet = CAnimate::new(self.id(), action);
        let exclude = if update_self { None } else { Some(self.id()) };
        world.broadcast_to_entity_trackers(self.id(), packet.clone(), exclude);
        if update_self && let Some(player) = self.as_player() {
            player.send_packet(packet);
        }
    }

    /// Runs vanilla `LivingEntity.updateSwingTime`.
    fn update_swing_time(&self) {
        self.living_base()
            .update_swing_time(self.current_swing_duration());
    }

    /// Returns a reference to this entity's attribute map.
    fn attributes(&self) -> &SyncMutex<AttributeMap> {
        self.living_base().attributes()
    }

    /// Appends vanilla-shaped living state used by command NBT predicates.
    fn save_command_nbt(&self, nbt: &mut NbtCompound) {
        nbt.insert("Health", self.get_health());
        nbt.insert(
            "DeathTime",
            NbtTag::Short(self.living_base().death_time() as i16),
        );
        nbt.insert("AbsorptionAmount", self.get_absorption_amount());
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

    /// Gets the current health of the entity.
    fn get_health(&self) -> f32;

    /// Sets the health of the entity, clamped between 0 and max health.
    fn set_health(&self, health: f32);

    /// Gets the maximum health from the attribute system.
    fn get_max_health(&self) -> f32 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::MAX_HEALTH) as f32
    }

    /// Returns vanilla `LivingEntity.noActionTime`.
    fn no_action_time(&self) -> i32 {
        self.living_base().no_action_time()
    }

    /// Sets vanilla `LivingEntity.noActionTime`.
    fn set_no_action_time(&self, no_action_time: i32) {
        self.living_base().set_no_action_time(no_action_time);
    }

    /// Increments vanilla `LivingEntity.noActionTime`.
    fn increment_no_action_time(&self) {
        self.living_base().increment_no_action_time();
    }

    /// Heals the entity by the specified amount.
    fn heal(&self, amount: f32) {
        let current_health = self.get_health();
        if current_health > 0.0 {
            self.set_health(current_health + amount);
        }
    }

    /// Returns true if the entity is dead or dying (health <= 0).
    fn is_dead_or_dying(&self) -> bool {
        self.get_health() <= 0.0
    }

    /// Returns vanilla `LivingEntity.isBaby()`.
    fn is_baby(&self) -> bool {
        false
    }

    /// Returns vanilla `LivingEntity.getSoundVolume`.
    fn sound_volume(&self) -> f32 {
        1.0
    }

    /// Returns vanilla `LivingEntity.getVoicePitch`.
    fn voice_pitch(&self) -> f32 {
        if self.is_baby() {
            (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.5
        } else {
            (rand::random::<f32>() - rand::random::<f32>()) * 0.2 + 1.0
        }
    }

    /// Returns vanilla `LivingEntity.getHurtSound`.
    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_GENERIC_HURT)
    }

    /// Returns vanilla `LivingEntity.getDeathSound`.
    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_GENERIC_DEATH)
    }

    /// Runs vanilla `LivingEntity.makeSound`.
    fn make_sound(&self, sound: Option<SoundEventRef>) {
        if let Some(sound) = sound {
            self.play_sound(sound, self.sound_volume(), self.voice_pitch());
        }
    }

    /// Runs vanilla `LivingEntity.playHurtSound`.
    fn play_hurt_sound(&self, source: &DamageSource) {
        if let Some(mob) = self.as_mob() {
            mob.reset_ambient_sound_time();
        }
        self.make_sound(self.hurt_sound(source));
    }

    /// Plays vanilla's death sound for this living entity.
    fn play_death_sound(&self) {
        self.make_sound(self.death_sound());
    }

    /// Returns vanilla `LivingEntity.getAgeScale()`.
    fn get_age_scale(&self) -> f32 {
        if self.is_baby() { 0.5 } else { 1.0 }
    }

    /// Returns vanilla `LivingEntity.getScale()`.
    fn get_scale(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::SCALE)
            .unwrap_or(1.0) as f32
    }

    /// Returns true if the entity is alive (health > 0).
    fn is_alive(&self) -> bool {
        !self.is_dead_or_dying()
    }

    /// Returns vanilla `LivingEntity.getArmorCoverPercentage()`.
    fn get_armor_cover_percentage(&self) -> f32 {
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

    /// Returns vanilla `LivingEntity.getVisibilityPercent()`.
    fn get_visibility_percent(&self, targeting_entity: Option<&dyn Entity>) -> f64 {
        let mut visibility_percent = 1.0;
        if self.is_discrete() {
            visibility_percent *= 0.8;
        }

        if self.is_invisible() {
            visibility_percent *= 0.7 * f64::from(self.get_armor_cover_percentage().max(0.1));
        }

        if self.disguise_head_matches_targeting_entity(targeting_entity) {
            visibility_percent *= 0.5;
        }

        visibility_percent
    }

    /// Returns whether the equipped head item reduces visibility to `targeting_entity`.
    fn disguise_head_matches_targeting_entity(
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

    /// Returns vanilla `LivingEntity.canBeSeenByAnyone()`.
    fn can_be_seen_by_anyone(&self) -> bool {
        !self.is_spectator() && Entity::is_alive(self)
    }

    /// Returns vanilla `LivingEntity.canBeSeenAsEnemy()`.
    fn can_be_seen_as_enemy(&self) -> bool {
        !self.is_invulnerable() && self.can_be_seen_by_anyone()
    }

    /// Returns vanilla `LivingEntity.canAttack()`.
    fn can_attack(&self, target: &dyn LivingEntity) -> bool {
        if target.entity_type() == &vanilla_entities::PLAYER
            && self
                .level()
                .is_some_and(|world| world.difficulty() == Difficulty::Peaceful)
        {
            return false;
        }

        target.can_be_seen_as_enemy()
    }

    /// Returns vanilla `LivingEntity.getLastDamageSource()`.
    fn last_damage_source(&self) -> Option<DamageSource> {
        let game_time = self.level().map_or(0, |world| world.game_time());
        self.living_base().last_damage_source(game_time)
    }

    /// Sets vanilla `LivingEntity.lastHurtByPlayer`.
    fn set_last_hurt_by_player(&self, player_uuid: Uuid, time_to_remember: i32) {
        self.living_base()
            .set_last_hurt_by_player(player_uuid, time_to_remember);
    }

    /// Returns vanilla `LivingEntity.lastHurtByPlayerMemoryTime`.
    fn last_hurt_by_player_memory_time(&self) -> i32 {
        self.living_base().last_hurt_by_player_memory_time()
    }

    /// Returns vanilla `LivingEntity.lastHurtByPlayer`, if still remembered.
    fn last_hurt_by_player_uuid(&self) -> Option<Uuid> {
        self.living_base().last_hurt_by_player_uuid()
    }

    /// Returns vanilla `LivingEntity.lastHurtByMob`.
    fn last_hurt_by_mob(&self) -> Option<SharedEntity> {
        self.living_base().last_hurt_by_mob()
    }

    /// Returns vanilla `LivingEntity.lastHurtByMobTimestamp`.
    fn last_hurt_by_mob_timestamp(&self) -> i32 {
        self.living_base().last_hurt_by_mob_timestamp()
    }

    /// Sets vanilla `LivingEntity.lastHurtByMob`.
    fn set_last_hurt_by_mob(&self, target: Option<&SharedEntity>) {
        self.living_base()
            .set_last_hurt_by_mob(target, self.tick_count());
    }

    /// Returns vanilla `LivingEntity.lastHurtMob`.
    fn last_hurt_mob(&self) -> Option<SharedEntity> {
        self.living_base().last_hurt_mob()
    }

    /// Returns vanilla `LivingEntity.lastHurtMobTimestamp`.
    fn last_hurt_mob_timestamp(&self) -> i32 {
        self.living_base().last_hurt_mob_timestamp()
    }

    /// Sets vanilla `LivingEntity.lastHurtMob`.
    fn set_last_hurt_mob(&self, target: Option<&SharedEntity>) {
        self.living_base()
            .set_last_hurt_mob(target, self.tick_count());
    }

    /// Resolves vanilla `LivingEntity.resolveMobResponsibleForDamage`.
    fn resolve_mob_responsible_for_damage(&self, world: &World, source: &DamageSource) {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_ANGER) {
            return;
        }
        if source.damage_type == &vanilla_damage_types::WIND_CHARGE
            && REGISTRY.entity_types.is_in_tag(
                self.entity_type(),
                &EntityTypeTag::NO_ANGER_FROM_WIND_CHARGE,
            )
        {
            return;
        }

        let Some(entity_id) = source.causing_entity_id else {
            return;
        };
        let Some(entity) = world.get_entity_by_id(entity_id) else {
            return;
        };
        if entity.is_living_entity() {
            self.set_last_hurt_by_mob(Some(&entity));
        }
    }

    /// Resolves vanilla `LivingEntity.resolvePlayerResponsibleForDamage`.
    fn resolve_player_responsible_for_damage(&self, world: &World, source: &DamageSource) {
        let Some(entity_id) = source.causing_entity_id else {
            return;
        };
        let Some(entity) = world.get_entity_by_id(entity_id) else {
            return;
        };
        if entity.entity_type() == &vanilla_entities::PLAYER {
            self.set_last_hurt_by_player(entity.uuid(), 100);
        }
    }

    /// Returns vanilla `LivingEntity.hasLineOfSight()`.
    fn has_line_of_sight(&self, target: &dyn Entity) -> bool {
        self.has_line_of_sight_with(
            target,
            ClipBlockShape::Collider,
            ClipFluid::None,
            target.get_eye_y(),
        )
    }

    /// Returns vanilla line-of-sight with explicit clip options.
    fn has_line_of_sight_with(
        &self,
        target: &dyn Entity,
        block_shape: ClipBlockShape,
        fluid: ClipFluid,
        target_eye_y: f64,
    ) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(target_world) = target.level() else {
            return false;
        };
        if !Arc::ptr_eq(&world, &target_world) {
            return false;
        }

        let position = self.position();
        let target_position = target.position();
        let start = DVec3::new(position.x, self.get_eye_y(), position.z);
        let end = DVec3::new(target_position.x, target_eye_y, target_position.z);
        if start.distance_squared(end) > 128.0 * 128.0 {
            return false;
        }

        world.clip(start, end, block_shape, fluid).is_miss()
    }

    /// Returns vanilla base living-entity invulnerability.
    fn default_is_invulnerable_to(&self, source: &DamageSource) -> bool {
        self.is_removed()
            || self.is_invulnerable() && !source.bypasses_invulnerability()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) && self.fire_immune()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
                && self.is_fall_damage_immune()
    }

    /// Returns whether this living entity ignores a damage source.
    fn is_invulnerable_to(&self, world: &World, source: &DamageSource) -> bool {
        self.default_is_invulnerable_to(source)
            || enchantment_helper::is_immune_to_damage(world, self, source)
    }

    /// Main vanilla living-entity damage entry point.
    ///
    /// `world` is the `ServerLevel` supplied by the vanilla caller. It may
    /// intentionally differ from the entity's attached world.
    fn hurt_server(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.is_invulnerable_to(world, source) {
            return false;
        }
        if self.is_dead_or_dying() {
            return false;
        }
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE)
            && self.has_mob_effect(vanilla_mob_effects::FIRE_RESISTANCE)
        {
            return false;
        }
        if self.is_sleeping() {
            self.stop_sleeping();
        }

        self.set_no_action_time(0);

        let mut damage = amount;
        if damage < 0.0 {
            damage = 0.0;
        }

        // TODO: apply item blocking before actually_hurt once shield/use-item hooks exist.
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FREEZING)
            && REGISTRY
                .entity_types
                .is_in_tag(self.entity_type(), &EntityTypeTag::FREEZE_HURTS_EXTRA_TYPES)
        {
            damage *= 5.0;
        }
        // TODO: apply helmet damage once those equipment hooks exist.
        if !damage.is_finite() {
            damage = f32::MAX;
        }

        let Some((took_full_damage, effective_amount)) = self
            .living_base()
            .apply_damage_cooldown(damage, source.bypasses_cooldown())
        else {
            return false;
        };

        self.before_actually_hurt(source, effective_amount);
        self.actually_hurt(world, source, effective_amount);
        self.resolve_mob_responsible_for_damage(world, source);
        self.resolve_player_responsible_for_damage(world, source);

        if took_full_damage {
            self.broadcast_damage_event(world, source);
            if !source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_IMPACT) {
                self.mark_hurt();
                self.broadcast_hurt_animation(world);
            }
            self.apply_damage_knockback(source);
        }

        if self.is_dead_or_dying() {
            if took_full_damage {
                self.play_death_sound();
            }
            self.die(source);
        } else if took_full_damage {
            self.play_hurt_sound(source);
        }
        // TODO: Play secondary hurt sounds once equipment effects expose them.

        let game_time = self.level().map_or(0, |world| world.game_time());
        self.living_base()
            .record_last_damage_source(source, game_time);

        true
    }

    /// Hook before applying damage after vanilla reductions.
    fn before_actually_hurt(&self, _source: &DamageSource, _amount: f32) {}

    /// Damages equipment that participates in vanilla armor absorption.
    fn hurt_armor(&self, _source: &DamageSource, _damage: f32) {}

    /// Mirrors vanilla `LivingEntity.doHurtEquipment`.
    fn do_hurt_equipment(&self, source: &DamageSource, damage: f32, slots: &[EquipmentSlot]) {
        if damage <= 0.0 {
            return;
        }

        let durability_damage = (damage / 4.0).max(1.0) as i32;
        for &slot in slots {
            let mut item_broke = false;
            self.with_equipment_slot_mut(slot, &mut |item| {
                let damage_on_hurt = item
                    .get_equippable()
                    .is_some_and(|equippable| equippable.damage_on_hurt);
                if damage_on_hurt
                    && item.is_damageable_item()
                    && item.can_be_hurt_by(source.damage_type)
                {
                    item_broke =
                        item.hurt_and_break(durability_damage, self.has_infinite_materials());
                }
            });
            if item_broke {
                self.on_equipped_item_broken(slot);
            }
        }
    }

    /// Mirrors vanilla `LivingEntity.getDamageAfterArmorAbsorb`.
    fn get_damage_after_armor_absorb(&self, source: &DamageSource, mut damage: f32) -> f32 {
        if !source.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_ARMOR) {
            self.hurt_armor(source, damage);
            let armor_toughness =
                self.attributes()
                    .lock()
                    .required_value(vanilla_attributes::ARMOR_TOUGHNESS) as f32;
            damage = combat_rules::get_damage_after_absorb(
                self,
                damage,
                source,
                self.get_armor_value() as f32,
                armor_toughness,
            );
        }
        damage
    }

    /// Mirrors vanilla `LivingEntity.getDamageAfterMagicAbsorb`.
    fn get_damage_after_magic_absorb(&self, source: &DamageSource, mut damage: f32) -> f32 {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_EFFECTS) {
            return damage;
        }

        if !source.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_RESISTANCE)
            && let Some(resistance) = self.mob_effect(vanilla_mob_effects::RESISTANCE)
        {
            let absorb_value = (resistance.amplifier() + 1) * 5;
            let absorb = 25 - absorb_value;
            damage = (damage * absorb as f32 / 25.0).max(0.0);
        }

        if damage <= 0.0 {
            return 0.0;
        }
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_ENCHANTMENTS) {
            return damage;
        }

        let enchantment_armor = self.level().map_or(0.0, |world| {
            enchantment_helper::get_damage_protection(&world, self, source)
        });
        if enchantment_armor > 0.0 {
            damage = combat_rules::get_damage_after_magic_absorb(damage, enchantment_armor);
        }
        damage
    }

    /// Applies damage after vanilla reductions.
    fn actually_hurt(&self, world: &World, source: &DamageSource, amount: f32) {
        if self.is_invulnerable_to(world, source) {
            return;
        }

        let damage = self.get_damage_after_armor_absorb(source, amount);
        let damage = self.get_damage_after_magic_absorb(source, damage);
        let original_damage = damage;
        let damage = (damage - self.get_absorption_amount()).max(0.0);
        self.set_absorption_amount(self.get_absorption_amount() - (original_damage - damage));

        if damage != 0.0 {
            self.set_health(self.get_health() - damage);
            self.set_absorption_amount(self.get_absorption_amount() - damage);
            self.game_event(&vanilla_game_events::ENTITY_DAMAGE);
        }
    }

    /// Applies vanilla hurt knockback for a damage source.
    fn apply_damage_knockback(&self, source: &DamageSource) {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_KNOCKBACK) {
            return;
        }

        let (xd, zd) = self.damage_knockback_direction(source);
        self.knockback(DAMAGE_KNOCKBACK_POWER, xd, zd);
        self.indicate_damage(xd, zd);
    }

    /// Returns the horizontal direction used by vanilla damage knockback.
    fn damage_knockback_direction(&self, source: &DamageSource) -> (f64, f64) {
        if let Some(direct_entity_id) = source.direct_entity_id
            && let Some(world) = self.level()
            && let Some(direct_entity) = world.get_entity_by_id(direct_entity_id)
            && let Some(projectile) = direct_entity.as_projectile()
            && let Some(hurt_entity) = self.as_living_entity()
        {
            let (xd, zd) =
                projectile.calculate_horizontal_hurt_knockback_direction(hurt_entity, source);
            return (-xd, -zd);
        }

        let Some(source_position) = source.source_position else {
            return (0.0, 0.0);
        };

        let position = self.position();
        (
            source_position.x - position.x,
            source_position.z - position.z,
        )
    }

    /// Applies vanilla `LivingEntity.knockback`.
    fn knockback(&self, mut power: f64, mut xd: f64, mut zd: f64) {
        power *= 1.0 - self.knockback_resistance();
        if power <= 0.0 {
            return;
        }

        while xd * xd + zd * zd < KNOCKBACK_DIRECTION_EPSILON_SQ {
            xd = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
            zd = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
        }

        let old_velocity = self.velocity();
        let delta_vector = DVec3::new(xd, 0.0, zd).normalize() * power;
        self.set_velocity(DVec3::new(
            old_velocity.x / 2.0 - delta_vector.x,
            if self.on_ground() {
                0.4_f64.min(old_velocity.y / 2.0 + power)
            } else {
                old_velocity.y
            },
            old_velocity.z / 2.0 - delta_vector.z,
        ));
        self.mark_velocity_sync();
    }

    /// Returns vanilla knockback resistance.
    fn knockback_resistance(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::KNOCKBACK_RESISTANCE)
    }

    /// Mirrors vanilla `LivingEntity.indicateDamage`.
    fn indicate_damage(&self, _xd: f64, _zd: f64) {}

    /// Returns the chunk used for vanilla nearby hurt broadcasts.
    fn hurt_broadcast_chunk(&self) -> ChunkPos {
        ChunkPos::from_entity_pos(self.position())
    }

    /// Broadcasts vanilla damage-event metadata near this entity.
    fn broadcast_damage_event(&self, world: &World, source: &DamageSource) {
        world.broadcast_to_nearby(
            self.hurt_broadcast_chunk(),
            CDamageEvent {
                entity_id: self.id(),
                source_type_id: source.damage_type.id() as i32,
                source_cause_id: source.causing_entity_id.map_or(0, |id| id + 1),
                source_direct_id: source.direct_entity_id.map_or(0, |id| id + 1),
                source_position: source.source_position,
            },
            None,
        );
    }

    /// Broadcasts vanilla hurt animation near this entity.
    fn broadcast_hurt_animation(&self, world: &World) {
        let (yaw, _) = self.rotation();
        world.broadcast_to_nearby(
            self.hurt_broadcast_chunk(),
            CHurtAnimation {
                entity_id: self.id(),
                yaw,
            },
            None,
        );
    }

    /// Processes vanilla living death side effects.
    fn die(&self, source: &DamageSource) {
        if self.is_removed() {
            return;
        }
        if !self.living_base().mark_death_processed() {
            return;
        }

        self.game_event(&vanilla_game_events::ENTITY_DIE);
        self.drop_all_death_loot(source);
        self.broadcast_entity_event(EntityStatus::Death);
        self.set_pose(EntityPose::Dying);
    }

    /// Returns vanilla `LivingEntity.shouldDropLoot`.
    fn should_drop_loot(&self, world: &World) -> bool {
        !self.is_baby() && world.get_game_rule(&MOB_DROPS)
    }

    /// Returns vanilla `LivingEntity.shouldDropExperience`.
    fn should_drop_experience(&self) -> bool {
        !self.is_baby()
    }

    /// Returns vanilla `LivingEntity.isAlwaysExperienceDropper`.
    fn is_always_experience_dropper(&self) -> bool {
        false
    }

    /// Runs vanilla `LivingEntity.skipDropExperience`.
    fn skip_drop_experience(&self) {
        self.living_base().skip_drop_experience();
    }

    /// Returns vanilla `LivingEntity.wasExperienceConsumed`.
    fn was_experience_consumed(&self) -> bool {
        self.living_base().was_experience_consumed()
    }

    /// Returns vanilla `LivingEntity.getBaseExperienceReward`.
    fn base_experience_reward(&self) -> i32 {
        if let Some(animal) = self.as_animal() {
            return animal.base_experience_reward_animal();
        }

        self.as_mob().map_or(0, Mob::base_experience_reward_mob)
    }

    /// Returns vanilla `LivingEntity.getExperienceReward`.
    fn experience_reward(&self, _world: &World, _killer_entity_id: Option<i32>) -> i32 {
        // TODO: Apply EnchantmentHelper.processMobExperience once enchantment
        // value-effect hooks can receive the killer/living-entity context.
        self.base_experience_reward()
    }

    /// Runs the currently implemented subset of vanilla `LivingEntity.dropAllDeathLoot`.
    fn drop_all_death_loot(&self, source: &DamageSource) {
        let Some(world) = self.level() else {
            return;
        };
        if self.should_drop_loot(world.as_ref()) {
            let killed_by_player = self.last_hurt_by_player_memory_time() > 0;
            self.drop_from_loot_table(source, killed_by_player);
            self.drop_custom_death_loot(source, killed_by_player);
            if let Some(mob) = self.as_mob() {
                mob.drop_custom_death_loot_mob(source, killed_by_player);
            }
        }
        self.drop_experience(&world, source.causing_entity_id);
        // TODO: Drop non-mob equipment overrides once those foundations exist.
    }

    /// Runs vanilla `LivingEntity.dropExperience`.
    fn drop_experience(&self, world: &Arc<World>, killer_entity_id: Option<i32>) {
        if self.was_experience_consumed() {
            return;
        }

        let should_drop = self.is_always_experience_dropper()
            || self.last_hurt_by_player_memory_time() > 0
                && self.should_drop_experience()
                && world.get_game_rule(&MOB_DROPS);
        if !should_drop {
            return;
        }

        let reward = self.experience_reward(world, killer_entity_id);
        if reward > 0 {
            ExperienceOrbEntity::award(world, self.position(), reward);
        }
    }

    /// Resolves the loot table used by vanilla `LivingEntity.dropFromLootTable`.
    fn death_loot_table(&self) -> Option<LootTableRef> {
        if let Some(mob) = self.as_mob()
            && mob.has_custom_death_loot_table()
        {
            return mob.custom_death_loot_table();
        }

        let entity_type = self.entity_type();
        let loot_key = Identifier::vanilla(format!("entities/{}", entity_type.key.path));
        REGISTRY.loot_tables.by_key(&loot_key)
    }

    /// Returns vanilla `Entity.getLootTableSeed` for death loot.
    fn death_loot_table_seed(&self) -> i64 {
        self.as_mob().map_or(0, Mob::death_loot_table_seed)
    }

    /// Runs vanilla `LivingEntity.dropFromLootTable`.
    fn drop_from_loot_table(&self, source: &DamageSource, killed_by_player: bool) {
        let Some(world) = self.level() else {
            return;
        };
        let has_custom_death_loot_table =
            self.as_mob().is_some_and(Mob::has_custom_death_loot_table);
        let Some(loot_table) = self.death_loot_table() else {
            if has_custom_death_loot_table && let Some(mob) = self.as_mob() {
                mob.clear_custom_death_loot_table();
            }
            return;
        };

        let seed = self.death_loot_table_seed();
        let drops = if seed == 0 {
            let mut rng = rand::rng();
            death_loot_items_with_rng(
                self,
                loot_table,
                world.as_ref(),
                source,
                killed_by_player,
                &mut rng,
            )
        } else {
            let mut rng = StdRng::seed_from_u64(seed as u64);
            death_loot_items_with_rng(
                self,
                loot_table,
                world.as_ref(),
                source,
                killed_by_player,
                &mut rng,
            )
        };

        if has_custom_death_loot_table && let Some(mob) = self.as_mob() {
            mob.clear_custom_death_loot_table();
        }

        for item_stack in drops {
            self.spawn_at_location(item_stack, 0.0);
        }
    }

    /// Hook for non-mob custom death loot.
    fn drop_custom_death_loot(&self, _source: &DamageSource, _killed_by_player: bool) {}

    /// Ticks the vanilla living death animation and removes the entity at completion.
    fn tick_death(&self) {
        let death_time = self.living_base().increment_death_time();
        if death_time >= DEATH_DURATION && !self.is_removed() {
            self.broadcast_entity_event(EntityStatus::Poof);
            self.set_removed(RemovalReason::Killed);
        }
    }

    /// Gets the absorption amount (extra health from effects like absorption).
    fn get_absorption_amount(&self) -> f32 {
        self.living_base().absorption_amount()
    }

    /// Sets the absorption amount.
    fn set_absorption_amount(&self, amount: f32) {
        self.living_base().set_absorption_amount(amount);
    }

    /// Returns vanilla `LivingEntity.getFallDamageSound()`.
    fn fall_damage_sound(&self, damage: i32) -> SoundEventRef {
        let (small, big) = self.fall_sounds();
        if damage > 4 { big } else { small }
    }

    /// Plays vanilla `LivingEntity.playBlockFallSound()`.
    fn play_block_fall_sound(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let position = self.position();
        let pos = BlockPos::new(
            position.x.floor() as i32,
            (position.y - f64::from(0.2_f32)).floor() as i32,
            position.z.floor() as i32,
        );
        let state = world.get_block_state(pos);
        if state.is_air() {
            return;
        }

        let sound_type = state.get_block().config.sound_type;
        self.play_sound(
            sound_type.fall_sound,
            sound_type.volume * 0.5,
            sound_type.pitch * 0.75,
        );
    }

    /// Mirrors vanilla `LivingEntity.causeFallDamage`.
    fn cause_living_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        let effective_fall_distance =
            if let Some(impact_pos) = self.living_base().current_impulse_impact_pos() {
                let effective_fall_distance = fall_distance.min(impact_pos.y - self.position().y);
                if effective_fall_distance <= 0.0 {
                    self.reset_current_impulse_context();
                } else {
                    self.try_reset_current_impulse_context();
                }
                effective_fall_distance
            } else {
                fall_distance
            };

        if self.is_fall_damage_immune() {
            return false;
        }

        self.propagate_fall_to_passengers(effective_fall_distance, damage_modifier, source);

        let attributes = self.attributes().lock();
        let safe_fall_distance = attributes
            .get_value(vanilla_attributes::SAFE_FALL_DISTANCE)
            .unwrap_or(vanilla_attributes::SAFE_FALL_DISTANCE.default_value);
        let fall_damage_multiplier = attributes
            .get_value(vanilla_attributes::FALL_DAMAGE_MULTIPLIER)
            .unwrap_or(vanilla_attributes::FALL_DAMAGE_MULTIPLIER.default_value);
        drop(attributes);

        let damage = LivingEntityBase::calculate_fall_damage(
            effective_fall_distance,
            damage_modifier,
            safe_fall_distance,
            fall_damage_multiplier,
        );
        if damage <= 0 {
            return false;
        }

        self.reset_current_impulse_context();
        self.play_sound(self.fall_damage_sound(damage), 1.0, 1.0);
        self.play_block_fall_sound();
        if let Some(world) = self.level() {
            self.hurt(&world, source, damage as f32);
        }
        true
    }

    /// Gets the entity's armor value from the attribute system.
    fn get_armor_value(&self) -> i32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::ARMOR)
            .unwrap_or(0.0) as i32
    }

    /// Gets the gravity value from the attribute system.
    fn get_attribute_gravity(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::GRAVITY)
    }

    /// Returns vanilla `LivingEntity.getEffectiveGravity()`.
    fn get_effective_gravity(&self) -> f64 {
        let gravity = self.get_gravity();
        if self.velocity().y <= 0.0 && self.has_mob_effect(vanilla_mob_effects::SLOW_FALLING) {
            gravity.min(0.01)
        } else {
            gravity
        }
    }

    /// Checks if the entity can be affected by potions.
    fn is_affected_by_potions(&self) -> bool {
        !self.is_dead_or_dying()
    }

    /// Returns vanilla base `LivingEntity.canBeAffected` eligibility.
    fn default_can_be_affected(&self, effect: &MobEffectInstance) -> bool {
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IMMUNE_TO_INFESTED)
        {
            return effect.effect() != vanilla_mob_effects::INFESTED;
        }
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IMMUNE_TO_OOZING)
        {
            return effect.effect() != vanilla_mob_effects::OOZING;
        }
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IGNORES_POISON_AND_REGEN)
        {
            return effect.effect() != vanilla_mob_effects::REGENERATION
                && effect.effect() != vanilla_mob_effects::POISON;
        }

        true
    }

    /// Returns whether this entity accepts a mob-effect instance.
    ///
    /// Concrete entities override this for vanilla class-specific immunities.
    fn can_be_affected(&self, effect: &MobEffectInstance) -> bool {
        self.default_can_be_affected(effect)
    }

    /// Returns vanilla `LivingEntity.hasEffect()`.
    fn has_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.living_base().has_mob_effect(effect)
    }

    /// Returns vanilla `LivingEntity.getEffect()`.
    fn mob_effect(&self, effect: MobEffectRef) -> Option<ActiveMobEffect> {
        self.living_base().mob_effect(effect)
    }

    /// Returns all active vanilla mob effects.
    fn active_mob_effects(&self) -> Vec<ActiveMobEffect> {
        self.living_base().active_mob_effects()
    }

    /// Sets active vanilla mob-effect state.
    fn set_mob_effect(&self, effect: MobEffectRef, amplifier: i32) {
        self.add_mob_effect(MobEffectInstance::new(effect, amplifier));
    }

    /// Adds or updates active vanilla mob-effect state.
    fn add_mob_effect(&self, effect: MobEffectInstance) -> bool {
        if !self.can_be_affected(&effect) {
            return false;
        }
        self.living_base().add_mob_effect(effect)
    }

    /// Sets the presence of a vanilla mob effect.
    fn set_mob_effect_active(&self, effect: MobEffectRef, active: bool) {
        if active {
            self.set_mob_effect(effect, 0);
        } else {
            self.remove_mob_effect(effect);
        }
    }

    /// Removes active vanilla mob-effect state.
    fn remove_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.living_base().remove_mob_effect(effect)
    }

    /// Ticks vanilla server-side mob-effect behavior and durations.
    fn tick_mob_effects(&self) {
        let world = self.level();
        for effect in self.active_mob_effects() {
            if !effect.has_remaining_duration() {
                self.living_base().tick_mob_effect_duration(effect.effect());
                continue;
            }

            if effect.should_apply_effect_tick_this_tick(self.tick_count())
                && world
                    .as_deref()
                    .is_some_and(|world| !effect.apply_effect_tick(world, self))
            {
                self.remove_mob_effect(effect.effect());
                continue;
            }

            self.living_base().tick_mob_effect_duration(effect.effect());
        }
    }

    /// Returns whether vanilla effects keep this entity from drowning.
    fn has_water_breathing(&self) -> bool {
        self.has_mob_effect(vanilla_mob_effects::WATER_BREATHING)
            || self.has_mob_effect(vanilla_mob_effects::CONDUIT_POWER)
            || self.has_mob_effect(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS)
    }

    /// Returns whether active vanilla effects refill this entity's air supply.
    fn should_effects_refill_air_supply(&self) -> bool {
        !self.has_mob_effect(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS)
            || self.has_mob_effect(vanilla_mob_effects::WATER_BREATHING)
            || self.has_mob_effect(vanilla_mob_effects::CONDUIT_POWER)
    }

    /// Returns vanilla `LivingEntity.canBreatheUnderwater`.
    fn can_breathe_underwater(&self) -> bool {
        self.entity_type().flags.can_breathe_underwater
    }

    /// Returns whether this entity can lose air and take drowning damage.
    fn can_drown_in_water(&self) -> bool {
        if self.can_breathe_underwater() || self.has_water_breathing() {
            return false;
        }

        !self
            .as_player()
            .is_some_and(|player| player.abilities.lock().invulnerable)
    }

    /// Returns whether the entity's eye block is a bubble column.
    fn is_eye_in_bubble_column(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        world
            .get_block_state(BlockPos::new(
                self.position().x.floor() as i32,
                self.get_eye_y().floor() as i32,
                self.position().z.floor() as i32,
            ))
            .get_block()
            == &vanilla_blocks::BUBBLE_COLUMN
    }

    /// Mirrors vanilla `LivingEntity.decreaseAirSupply`.
    fn decrease_air_supply(&self, current_supply: i32) -> i32 {
        let oxygen_bonus = self
            .attributes()
            .lock()
            .get_value(vanilla_attributes::OXYGEN_BONUS)
            .unwrap_or(0.0);
        if oxygen_bonus > 0.0 && rand::random::<f64>() >= 1.0 / (oxygen_bonus + 1.0) {
            current_supply
        } else {
            current_supply - 1
        }
    }

    /// Mirrors vanilla `LivingEntity.increaseAirSupply`.
    fn increase_air_supply(&self, current_supply: i32) -> i32 {
        (current_supply + 4).min(self.max_air_supply())
    }

    /// Mirrors vanilla `LivingEntity.shouldTakeDrowningDamage`.
    fn should_take_drowning_damage(&self) -> bool {
        self.air_supply() <= -20
    }

    /// Ticks vanilla living air-supply and drowning behavior from `baseTick`.
    fn tick_living_air_supply(&self) {
        if !LivingEntity::is_alive(self) {
            return;
        }

        let eye_in_water = self.is_eye_in_water() && !self.is_eye_in_bubble_column();
        if eye_in_water {
            if self.can_drown_in_water() {
                self.set_air_supply(self.decrease_air_supply(self.air_supply()));
                if self.should_take_drowning_damage() {
                    self.set_air_supply(0);
                    self.broadcast_entity_event(EntityStatus::DrownParticles);
                    if let Some(world) = self.level() {
                        self.hurt(
                            &world,
                            &DamageSource::environment(&vanilla_damage_types::DROWN),
                            2.0,
                        );
                    }
                }
            } else if self.air_supply() < self.max_air_supply()
                && self.should_effects_refill_air_supply()
            {
                self.set_air_supply(self.increase_air_supply(self.air_supply()));
            }

            if self
                .vehicle()
                .is_some_and(|vehicle| vehicle.dismounts_underwater())
            {
                self.stop_riding();
            }
            return;
        }

        if self.air_supply() < self.max_air_supply() {
            self.set_air_supply(self.increase_air_supply(self.air_supply()));
        }
    }

    /// Mirrors vanilla `LivingEntity.isInWall`.
    fn is_in_wall(&self) -> bool {
        !self.is_sleeping() && Entity::is_in_wall(self)
    }

    /// Applies vanilla living in-wall damage from `baseTick`.
    fn tick_in_wall_damage(&self) {
        if !LivingEntity::is_alive(self) || !LivingEntity::is_in_wall(self) {
            return;
        }

        if let Some(world) = self.level() {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::IN_WALL),
                1.0,
            );
        }
    }

    /// Applies vanilla living environmental damage in `LivingEntity.baseTick` order.
    fn tick_living_environmental_damage(&self) {
        if !LivingEntity::is_alive(self) {
            return;
        }

        if LivingEntity::is_in_wall(self) {
            self.tick_in_wall_damage();
        } else if self.as_player().is_some()
            && let Some(world) = self.level()
        {
            let border = world.world_border_snapshot();
            let position = self.position();
            if let Some(damage) =
                border.outside_damage_amount(position.x, position.z, self.bounding_box())
            {
                self.hurt(
                    &world,
                    &DamageSource::environment(&vanilla_damage_types::OUTSIDE_BORDER),
                    damage,
                );
            }
        }

        self.tick_living_air_supply();
    }

    /// Returns vanilla `LivingEntity.isAffectedByFluids()`.
    fn is_affected_by_fluids(&self) -> bool {
        true
    }

    /// Returns vanilla `LivingEntity.canStandOnFluid()`.
    fn can_stand_on_fluid(&self, _fluid_state: FluidState) -> bool {
        false
    }

    /// Checks if the entity is currently using an item.
    fn is_using_item(&self) -> bool {
        false
    }

    /// Checks if the entity is blocking with a shield or similar item.
    fn is_blocking(&self) -> bool {
        false
    }

    /// Checks if the entity is fall flying (using elytra).
    fn is_fall_flying(&self) -> bool {
        self.living_base().is_fall_flying()
    }

    /// Sets whether this entity is fall flying.
    fn set_fall_flying(&self, fall_flying: bool) {
        self.set_shared_fall_flying(fall_flying);
        self.living_base().set_fall_flying(fall_flying);
    }

    /// Returns vanilla `LivingEntity.getFallFlyingTicks()`.
    fn fall_flying_ticks(&self) -> i32 {
        self.living_base().fall_flying_ticks()
    }

    /// Visits the item in a vanilla living-entity equipment slot.
    fn with_equipment_slot(&self, slot: EquipmentSlot, visitor: &mut dyn FnMut(&ItemStack)) {
        let equipment = self.living_base().equipment().lock();
        visitor(equipment.get_ref(slot));
    }

    /// Returns vanilla `LivingEntity.isHolding`.
    fn is_holding(&self, predicate: &mut dyn FnMut(&ItemStack) -> bool) -> bool {
        let mut holding = false;
        self.with_equipment_slot(EquipmentSlot::MainHand, &mut |item_stack| {
            holding = predicate(item_stack);
        });
        if holding {
            return true;
        }

        self.with_equipment_slot(EquipmentSlot::OffHand, &mut |item_stack| {
            holding = predicate(item_stack);
        });
        holding
    }

    /// Mutates the item in a vanilla living-entity equipment slot.
    fn with_equipment_slot_mut(
        &self,
        slot: EquipmentSlot,
        visitor: &mut dyn FnMut(&mut ItemStack),
    ) {
        let mut equipment = self.living_base().equipment().lock();
        visitor(equipment.get_mut(slot));
    }

    /// Returns whether this entity currently has an item in `slot`.
    fn has_item_in_slot(&self, slot: EquipmentSlot) -> bool {
        let mut has_item = false;
        self.with_equipment_slot(slot, &mut |item_stack| {
            has_item = !item_stack.is_empty();
        });
        has_item
    }

    /// Returns whether vanilla allows this entity to use `slot`.
    fn can_use_slot(&self, _slot: EquipmentSlot) -> bool {
        true
    }

    /// Returns the effective vanilla dispenser slot gate for living entities and mobs.
    fn can_dispenser_equip_into_slot(&self, _slot: EquipmentSlot) -> bool {
        self.as_mob().is_none_or(Mob::can_pick_up_loot)
    }

    /// Returns vanilla `LivingEntity.canEquipWithDispenser`.
    fn can_equip_with_dispenser(&self, item_stack: &ItemStack) -> bool {
        if !Entity::is_alive(self) || self.is_spectator() {
            return false;
        }

        let Some(equippable) = item_stack.get_equippable() else {
            return false;
        };
        if !equippable.dispensable {
            return false;
        }

        let slot = equippable.slot;
        self.can_use_slot(slot)
            && equippable.can_be_equipped_by(self.entity_type())
            && !self.has_item_in_slot(slot)
            && self.can_dispenser_equip_into_slot(slot)
    }

    /// Returns vanilla `LivingEntity.isEquippableInSlot`.
    fn is_equippable_in_slot(&self, item_stack: &ItemStack, slot: EquipmentSlot) -> bool {
        let Some(equippable) = item_stack.get_equippable() else {
            return slot == EquipmentSlot::MainHand && self.can_use_slot(EquipmentSlot::MainHand);
        };

        slot == equippable.slot
            && self.can_use_slot(equippable.slot)
            && equippable.can_be_equipped_by(self.entity_type())
    }

    /// Returns the equip sound Steel can currently resolve for this entity.
    fn equip_sound(&self, slot: EquipmentSlot, stack: &ItemStack) -> Option<SoundEventRef> {
        let equippable = stack.get_equippable()?;
        (slot == equippable.slot)
            .then(|| equippable.equip_sound.registry_ref())
            .flatten()
    }

    /// Runs vanilla's equippable `ItemStack.interactLivingEntity` branch.
    fn interact_living_entity_with_equippable(
        &self,
        player: &Player,
        hand: InteractionHand,
    ) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.copy_with_count(item_stack.count())
        };
        let Some(equippable) = item_stack.get_equippable() else {
            return InteractionResult::Pass;
        };
        if !equippable.equip_on_interact {
            return InteractionResult::Pass;
        }

        let slot = equippable.slot;
        if !self.is_equippable_in_slot(&item_stack, slot) || !Entity::is_alive(self) {
            return InteractionResult::Pass;
        }

        let equipped = {
            let mut equipment = self.living_base().equipment().lock();
            if !equipment.get_ref(slot).is_empty() {
                return InteractionResult::Pass;
            }

            let mut inventory = player.inventory.lock();
            if !self.is_equippable_in_slot(inventory.get_item_in_hand(hand), slot) {
                return InteractionResult::Pass;
            }

            let equipped = inventory.split_item_in_hand(hand, 1);
            if equipped.is_empty() {
                return InteractionResult::Pass;
            }

            equipment.set(slot, equipped);
            equipment.get_ref(slot).copy_with_count(1)
        };

        self.refresh_equipment_attribute_modifiers(slot);
        if let Some(sound) = self.equip_sound(slot, &equipped) {
            self.play_sound(sound, 1.0, 1.0);
        }
        if let Some(mob) = self.as_mob() {
            mob.set_guaranteed_drop(slot);
        }
        // TODO: Emit EQUIP game event once game-event dispatch is implemented.
        InteractionResult::Success
    }

    /// Refreshes transient item attribute modifiers for one equipment slot.
    fn refresh_equipment_attribute_modifiers(&self, slot: EquipmentSlot) {
        self.with_equipment_slot(slot, &mut |item_stack| {
            self.living_base()
                .refresh_equipment_attribute_modifiers(slot, item_stack);
        });
    }

    /// Refreshes transient item attribute modifiers for all equipment slots.
    fn refresh_all_equipment_attribute_modifiers(&self) {
        for slot in EquipmentSlot::ALL {
            self.refresh_equipment_attribute_modifiers(slot);
        }
    }

    /// Packs non-empty living equipment slots for initial spawn pairing.
    fn pack_living_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.living_base().equipment().lock().non_empty_items())
    }

    /// Drains dirty living equipment slots for tracker sync.
    fn drain_dirty_living_equipment(&self) -> Vec<EquipmentSlotItem> {
        equipment_items_to_packet_items(self.living_base().equipment().lock().drain_dirty_items())
    }

    /// Returns whether equipment durability should be skipped for this entity.
    fn has_infinite_materials(&self) -> bool {
        false
    }

    /// Called after an equipped item breaks.
    fn on_equipped_item_broken(&self, slot: EquipmentSlot) {
        let event = match slot {
            EquipmentSlot::MainHand => EntityStatus::MainhandBreak,
            EquipmentSlot::OffHand => EntityStatus::OffhandBreak,
            EquipmentSlot::Head => EntityStatus::HeadBreak,
            EquipmentSlot::Chest => EntityStatus::ChestBreak,
            EquipmentSlot::Legs => EntityStatus::LegsBreak,
            EquipmentSlot::Feet => EntityStatus::FeetBreak,
            EquipmentSlot::Body => EntityStatus::BodyBreak,
            EquipmentSlot::Saddle => EntityStatus::SaddleBreak,
        };
        self.broadcast_entity_event(event);
        self.refresh_equipment_attribute_modifiers(slot);
    }

    /// Returns vanilla `LivingEntity.canFreeze()` after concrete entity exemptions.
    ///
    /// Vanilla keeps the entity-type freeze immunity on `Entity` and the equipment
    /// immunity on `LivingEntity`. Steel keeps this helper separate so concrete
    /// `Entity::can_freeze` implementations can delegate without downcasting.
    fn default_living_can_freeze(&self) -> bool {
        for slot in EquipmentSlot::ALL {
            if !slot.is_armor() {
                continue;
            }
            let mut is_freeze_immune = false;
            self.with_equipment_slot(slot, &mut |item_stack| {
                is_freeze_immune = REGISTRY
                    .items
                    .is_in_tag(item_stack.item(), &ItemTag::FREEZE_IMMUNE_WEARABLES);
            });

            if is_freeze_immune {
                return false;
            }
        }

        self.default_can_freeze()
    }

    /// Returns whether vanilla `tryAddFrost` sees a non-air block below.
    fn is_on_non_air_block_for_frost(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(pos) = self.on_pos_legacy() else {
            return false;
        };

        world.get_block_state(pos).get_block() != &vanilla_blocks::AIR
    }

    /// Mirrors vanilla `LivingEntity.removeFrost`.
    fn remove_frost(&self) {
        self.attributes().lock().remove_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            &SPEED_MODIFIER_POWDER_SNOW_ID,
        );
    }

    /// Mirrors vanilla `LivingEntity.tryAddFrost`.
    fn try_add_frost(&self) {
        if !self.is_on_non_air_block_for_frost() || self.ticks_frozen() <= 0 {
            return;
        }

        self.attributes().lock().add_modifier(
            vanilla_attributes::MOVEMENT_SPEED,
            AttributeModifier {
                id: SPEED_MODIFIER_POWDER_SNOW_ID,
                amount: f64::from(-0.05_f32 * self.percent_frozen()),
                operation: AttributeModifierOperation::AddValue,
            },
            false,
        );
    }

    /// Ticks vanilla `LivingEntity.aiStep` freezing effects.
    fn tick_freezing(&self) {
        if !self.is_in_powder_snow() || !self.can_freeze() {
            self.set_ticks_frozen((self.ticks_frozen() - 2).max(0));
        }

        self.remove_frost();
        self.try_add_frost();
        if self.tick_count() % 40 == 0
            && self.is_fully_frozen()
            && self.can_freeze()
            && let Some(world) = self.level()
        {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::FREEZE),
                1.0,
            );
        }
    }

    /// Returns vanilla `PowderSnowBlock.canEntityWalkOnPowderSnow()` for living entities.
    fn default_living_can_walk_on_powder_snow(&self) -> bool {
        if self.default_can_walk_on_powder_snow() {
            return true;
        }

        let mut has_leather_boots = false;
        self.with_equipment_slot(EquipmentSlot::Feet, &mut |item_stack| {
            has_leather_boots = item_stack.is(&vanilla_items::LEATHER_BOOTS);
        });
        has_leather_boots
    }

    /// Ticks living-entity counters after movement.
    fn tick_living_state(&self) {
        if let Some(mob) = self.as_mob() {
            mob.tick_body_rotation_control();
        }
        self.living_base()
            .tick_fall_flying_state(self.is_fall_flying());
        self.update_swing_time();
        self.refresh_dirty_attributes();
        self.living_base().tick_post_impulse_grace_time();
        self.living_base().tick_last_hurt_by_player_memory();
        self.living_base()
            .tick_living_combat_memory(self.tick_count());
    }

    /// Mirrors vanilla `LivingEntity.canGlideUsing()`.
    fn can_glide_using(&self, item_stack: &ItemStack, slot: EquipmentSlot) -> bool {
        let Some(equippable) = item_stack.get_equippable() else {
            return false;
        };

        item_stack.has(GLIDER) && equippable.slot == slot && !item_stack.next_damage_will_break()
    }

    /// Returns whether the item in `slot` can be used for vanilla gliding.
    fn can_glide_using_equipment_slot(&self, slot: EquipmentSlot) -> bool {
        let mut can_glide = false;
        self.with_equipment_slot(slot, &mut |item_stack| {
            can_glide = self.can_glide_using(item_stack, slot);
        });
        can_glide
    }

    /// Damages one random equipped glider like vanilla `LivingEntity.updateFallFlying()`.
    fn damage_random_glider(&self) {
        let mut slots_with_gliders = Vec::new();
        for slot in EquipmentSlot::ALL {
            if self.can_glide_using_equipment_slot(slot) {
                slots_with_gliders.push(slot);
            }
        }

        let slot_count = slots_with_gliders.len();
        if slot_count == 0 {
            return;
        }

        let slot_index = rand::random_range(0..slot_count);
        let slot_to_damage = slots_with_gliders[slot_index];
        let has_infinite_materials = self.has_infinite_materials();
        let mut item_broke = false;
        self.with_equipment_slot_mut(slot_to_damage, &mut |item_stack| {
            item_broke = item_stack.hurt_and_break(1, has_infinite_materials);
        });
        if item_broke {
            self.on_equipped_item_broken(slot_to_damage);
        }
    }

    /// Default vanilla `LivingEntity.canGlide()` implementation for overrides.
    fn default_can_glide(&self) -> bool {
        !self.on_ground()
            && !self.is_passenger()
            && !self.has_mob_effect(vanilla_mob_effects::LEVITATION)
            && EquipmentSlot::ALL
                .iter()
                .any(|&slot| self.can_glide_using_equipment_slot(slot))
    }

    /// Mirrors vanilla `LivingEntity.canGlide()`.
    fn can_glide(&self) -> bool {
        self.default_can_glide()
    }

    /// Mirrors vanilla `Player.startFallFlying()`.
    fn start_fall_flying(&self) {
        self.set_fall_flying(true);
    }

    /// Mirrors vanilla `Player.tryToStartFallFlying()`.
    fn try_to_start_fall_flying(&self) -> bool {
        if !self.is_fall_flying() && self.can_glide() && !self.is_in_water() {
            self.start_fall_flying();
            return true;
        }

        false
    }

    /// Returns the last climbable block position this living entity touched.
    fn last_climbable_pos(&self) -> Option<BlockPos> {
        self.living_base().last_climbable_pos()
    }

    /// Records the last climbable block position this living entity touched.
    fn set_last_climbable_pos(&self, pos: BlockPos) {
        self.living_base().set_last_climbable_pos(pos);
    }

    /// Returns vanilla `LivingEntity.onClimbable()` behavior.
    fn default_living_on_climbable(&self) -> bool {
        if self.is_spectator() {
            return false;
        }

        let pos = self.block_position();
        let Some(world) = self.level() else {
            return false;
        };
        let state = world.get_block_state(pos);
        let block = state.get_block();

        if self.is_fall_flying() && block.has_tag(&BlockTag::CAN_GLIDE_THROUGH) {
            return false;
        }

        let climbable = block.has_tag(&BlockTag::CLIMBABLE)
            || block.has_tag(&BlockTag::TRAPDOORS)
                && trapdoor_usable_as_ladder_state(state, world.get_block_state(pos.below()));

        if climbable {
            self.set_last_climbable_pos(pos);
        }

        climbable
    }

    /// Returns whether vanilla living travel should skip friction damping.
    fn should_discard_friction(&self) -> bool {
        self.living_base().should_discard_friction()
    }

    /// Sets whether vanilla living travel should skip friction damping.
    fn set_discard_friction(&self, discard_friction: bool) {
        self.living_base().set_discard_friction(discard_friction);
    }

    /// Returns whether this living entity is currently applying jump input.
    fn is_jumping(&self) -> bool {
        self.living_base().is_jumping()
    }

    /// Sets whether this living entity is currently applying jump input.
    fn set_jumping(&self, jumping: bool) {
        self.living_base().set_jumping(jumping);
    }

    /// Returns vanilla living travel input.
    fn travel_input(&self) -> LivingTravelInput {
        self.living_base().travel_input()
    }

    /// Sets vanilla living travel input.
    fn set_travel_input(&self, input: LivingTravelInput) {
        self.living_base().set_travel_input(input);
    }

    /// Applies vanilla `LivingEntity.applyInput()` damping.
    fn apply_input(&self) {
        self.living_base().dampen_travel_input();
    }

    /// Returns vanilla jump cooldown ticks.
    fn no_jump_delay(&self) -> i32 {
        self.living_base().no_jump_delay()
    }

    /// Sets vanilla jump cooldown ticks.
    fn set_no_jump_delay(&self, ticks: i32) {
        self.living_base().set_no_jump_delay(ticks);
    }

    /// Decrements vanilla jump cooldown once per living AI step.
    fn tick_no_jump_delay(&self) {
        self.living_base().tick_no_jump_delay();
    }

    /// Returns vanilla `LivingEntity.isImmobile()`.
    fn default_is_immobile(&self) -> bool {
        self.is_dead_or_dying()
    }

    /// Returns vanilla `LivingEntity.isImmobile()`.
    fn is_immobile(&self) -> bool {
        self.default_is_immobile()
    }

    /// Applies vanilla `LivingEntity.aiStep()` velocity thresholds.
    fn apply_living_velocity_thresholds(&self) {
        let movement = self.velocity();
        let mut dx = movement.x;
        let mut dy = movement.y;
        let mut dz = movement.z;

        if self.entity_type() == &vanilla_entities::PLAYER {
            if movement.x.mul_add(movement.x, movement.z * movement.z) < 9.0E-6 {
                dx = 0.0;
                dz = 0.0;
            }
        } else {
            if movement.x.abs() < 0.003 {
                dx = 0.0;
            }
            if movement.z.abs() < 0.003 {
                dz = 0.0;
            }
        }

        if movement.y.abs() < 0.003 {
            dy = 0.0;
        }

        self.set_velocity(DVec3::new(dx, dy, dz));
    }

    /// Server AI hook called from vanilla `LivingEntity.aiStep()`.
    fn server_ai_step(&self) {}

    /// Returns vanilla `LivingEntity.getJumpBoostPower()`.
    fn get_jump_boost_power(&self) -> f32 {
        self.mob_effect(vanilla_mob_effects::JUMP_BOOST)
            .map_or(0.0, |effect| 0.1 * (effect.amplifier() as f32 + 1.0))
    }

    /// Returns vanilla `LivingEntity.getJumpPower(float)`.
    fn get_jump_power_with_multiplier(&self, multiplier: f32) -> f32 {
        let jump_strength =
            self.attributes()
                .lock()
                .get_value(vanilla_attributes::JUMP_STRENGTH)
                .unwrap_or(vanilla_attributes::JUMP_STRENGTH.default_value) as f32;
        jump_strength * multiplier * self.block_jump_factor() + self.get_jump_boost_power()
    }

    /// Returns vanilla `LivingEntity.getJumpPower()`.
    fn get_jump_power(&self) -> f32 {
        self.get_jump_power_with_multiplier(1.0)
    }

    /// Default vanilla `LivingEntity.jumpFromGround()` implementation for overrides.
    fn default_jump_from_ground(&self) {
        let jump_power = self.get_jump_power();
        if jump_power <= 1.0E-5 {
            return;
        }

        let movement = self.velocity();
        self.set_velocity(DVec3::new(
            movement.x,
            movement.y.max(f64::from(jump_power)),
            movement.z,
        ));
        if self.is_sprinting() {
            let angle = self.rotation().0.to_radians();
            self.set_velocity(
                self.velocity()
                    + DVec3::new(
                        f64::from(-angle.sin() * 0.2),
                        0.0,
                        f64::from(angle.cos() * 0.2),
                    ),
            );
        }

        self.mark_velocity_sync();
    }

    /// Mirrors vanilla `LivingEntity.jumpFromGround()`.
    fn jump_from_ground(&self) {
        self.default_jump_from_ground();
    }

    /// Mirrors vanilla `LivingEntity.goDownInWater()`.
    fn go_down_in_water(&self) {
        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(-0.04_f32), 0.0));
    }

    /// Mirrors vanilla `LivingEntity.jumpInLiquid()`.
    fn jump_in_liquid(&self, _fluid_tag: &Identifier) {
        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    }

    /// Applies vanilla `LivingEntity.aiStep()` jump handling.
    fn handle_living_jump(&self) {
        if !self.is_jumping() || !self.is_affected_by_fluids() {
            self.set_no_jump_delay(0);
            return;
        }

        let fluid_height = if self.is_in_lava() {
            self.fluid_contact().lava_height()
        } else {
            self.fluid_contact().water_height()
        };
        let in_water_and_has_fluid_height = self.is_in_water() && fluid_height > 0.0;
        let fluid_jump_threshold = self.get_fluid_jump_threshold();
        if !in_water_and_has_fluid_height
            || self.on_ground() && fluid_height <= fluid_jump_threshold
        {
            if !self.is_in_lava() || self.on_ground() && fluid_height <= fluid_jump_threshold {
                if (self.on_ground()
                    || in_water_and_has_fluid_height && fluid_height <= fluid_jump_threshold)
                    && self.no_jump_delay() == 0
                {
                    self.jump_from_ground();
                    self.set_no_jump_delay(10);
                }
            } else {
                self.jump_in_liquid(&vanilla_fluid_tags::FluidTag::LAVA);
            }
        } else {
            self.jump_in_liquid(&vanilla_fluid_tags::FluidTag::WATER);
        }
    }

    /// Mirrors vanilla `LivingEntity.tickRidden()`.
    fn tick_ridden(&self, _controller: &Player, _ridden_input: DVec3) {}

    /// Mirrors vanilla `LivingEntity.getRiddenInput()`.
    fn ridden_input(&self, _controller: &Player, self_input: DVec3) -> DVec3 {
        self_input
    }

    /// Mirrors vanilla `LivingEntity.getRiddenSpeed()`.
    fn ridden_speed(&self, _controller: &Player) -> f32 {
        self.get_speed()
    }

    /// Mirrors vanilla `LivingEntity.travelRidden()`.
    fn travel_ridden(&self, controller: &Player, self_input: DVec3) -> Option<MoveResult> {
        let ridden_input = self.ridden_input(controller, self_input);
        self.tick_ridden(controller, ridden_input);
        if self.can_simulate_movement() {
            self.set_speed(self.ridden_speed(controller));
            return self.travel(ridden_input);
        }

        self.set_velocity(DVec3::ZERO);
        None
    }

    /// Default vanilla-shaped `LivingEntity.aiStep()` movement foundation for overrides.
    ///
    /// This covers the shared travel state Steel currently has; mob AI and
    /// equipment ticking are still separate follow-up work.
    fn default_ai_step(&self) -> Option<MoveResult> {
        self.tick_no_jump_delay();
        if !self.can_simulate_movement() {
            self.set_velocity(self.velocity() * 0.98);
        }

        self.apply_living_velocity_thresholds();
        self.apply_input();
        if self.is_immobile() {
            self.set_jumping(false);
            let input = self.travel_input();
            self.set_travel_input(LivingTravelInput::new(0.0, input.vertical(), 0.0));
        } else if self.is_effective_ai() {
            self.server_ai_step();
        }

        self.handle_living_jump();

        if self.is_fall_flying() {
            self.update_fall_flying();
        }

        if self.has_mob_effect(vanilla_mob_effects::SLOW_FALLING)
            || self.has_mob_effect(vanilla_mob_effects::LEVITATION)
        {
            self.reset_fall_distance();
        }

        let input = self.travel_input();
        let input = DVec3::new(
            f64::from(input.sideways()),
            f64::from(input.vertical()),
            f64::from(input.forward()),
        );
        let result = if Entity::is_alive(self)
            && let Some(controller_entity) = self.controlling_passenger()
            && let Some(controller) = controller_entity.as_player()
        {
            self.travel_ridden(controller, input)
        } else if self.can_simulate_movement() && self.is_effective_ai() {
            self.travel(input)
        } else {
            None
        };

        self.apply_effects_from_blocks();
        self.tick_freezing();
        self.push_entities();
        result
    }

    /// Mirrors vanilla `LivingEntity.aiStep()`.
    fn ai_step(&self) -> Option<MoveResult> {
        self.default_ai_step()
    }

    /// Mirrors vanilla `LivingEntity.pushEntities()`.
    fn push_entities(&self) {
        let Some(world) = self.level() else {
            return;
        };
        if !world.tick_runs_normally() {
            return;
        }

        let pusher = self.as_entity_event_source();
        let pushable_entities = world.get_pushable_entities(pusher, &self.bounding_box());
        if pushable_entities.is_empty() {
            return;
        }

        self.apply_entity_cramming_damage(&world, &pushable_entities);

        for entity in pushable_entities {
            entity.push_entity(pusher);
        }
    }

    /// Applies vanilla max entity cramming damage from `LivingEntity.pushEntities()`.
    fn apply_entity_cramming_damage(&self, world: &World, pushable_entities: &[SharedEntity]) {
        let max_cramming = world.get_game_rule(&MAX_ENTITY_CRAMMING);

        if max_cramming <= 0 || pushable_entities.len() <= (max_cramming - 1) as usize {
            return;
        }

        let random_roll = rand::random_range(0..4);
        let non_passenger_count = pushable_entities
            .iter()
            .filter(|entity| !entity.is_passenger())
            .count();

        if should_apply_entity_cramming_damage(
            max_cramming,
            pushable_entities.len(),
            non_passenger_count,
            random_roll,
        ) {
            self.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::CRAMMING),
                6.0,
            );
        }
    }

    /// Returns vanilla `LivingEntity.isSuppressingSlidingDownLadder()`.
    fn is_suppressing_sliding_down_ladder(&self) -> bool {
        self.is_suppressing_bounce()
    }

    /// Returns a levitation velocity adjustment for `travelInAir`.
    fn levitation_travel_y_delta(&self, movement_y: f64) -> Option<f64> {
        self.mob_effect(vanilla_mob_effects::LEVITATION)
            .map(|effect| (0.05 * f64::from(effect.amplifier() + 1) - movement_y) * 0.2)
    }

    /// Returns whether vanilla `LivingEntity.travel()` should use fluid movement.
    fn should_travel_in_fluid(&self, fluid_state: FluidState) -> bool {
        (self.is_in_water() || self.is_in_lava())
            && self.is_affected_by_fluids()
            && !self.can_stand_on_fluid(fluid_state)
    }

    /// Returns vanilla `LivingEntity.getWaterSlowDown()`.
    fn get_water_slow_down(&self) -> f32 {
        0.8
    }

    /// Returns the water movement efficiency attribute used by fluid travel.
    fn water_movement_efficiency(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::WATER_MOVEMENT_EFFICIENCY)
            .unwrap_or(0.0) as f32
    }

    /// Returns whether dolphin's grace should apply to water travel.
    fn has_dolphins_grace(&self) -> bool {
        self.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE)
    }

    /// Returns vanilla `LivingEntity.getFlyingSpeed()`.
    fn get_flying_speed(&self) -> f32 {
        if self
            .controlling_passenger()
            .is_some_and(|passenger| passenger.entity_type() == &vanilla_entities::PLAYER)
        {
            self.get_speed() * 0.1
        } else {
            0.02
        }
    }

    /// Returns vanilla `LivingEntity.getFrictionInfluencedSpeed()`.
    fn get_friction_influenced_speed(&self, block_friction: f32) -> f32 {
        if self.on_ground() {
            self.get_speed() * (0.216_000_02 / (block_friction * block_friction * block_friction))
        } else {
            self.get_flying_speed()
        }
    }

    /// Returns the vertical friction used by `travelInAir`.
    fn air_travel_vertical_friction(&self, _horizontal_friction: f32) -> f32 {
        // TODO: FlyingAnimal uses horizontal friction here once animal types exist.
        0.98
    }

    /// Applies vanilla `LivingEntity.handleOnClimbable()`.
    fn handle_on_climbable(&self, movement: DVec3) -> DVec3 {
        if !self.on_climbable() {
            return movement;
        }

        self.reset_fall_distance();
        let Some(world) = self.level() else {
            return movement;
        };
        let block_state = self.in_block_state(&world);
        let mut y = movement.y.max(-0.15);
        if y < 0.0
            && block_state.get_block() != &vanilla_blocks::SCAFFOLDING
            && self.is_suppressing_sliding_down_ladder()
            && self.entity_type() == &vanilla_entities::PLAYER
        {
            y = 0.0;
        }

        DVec3::new(
            movement.x.clamp(-0.15, 0.15),
            y,
            movement.z.clamp(-0.15, 0.15),
        )
    }

    /// Applies gravity using vanilla living-entity effective gravity.
    fn apply_living_travel_gravity(&self) {
        let gravity = self.get_effective_gravity();
        if gravity != 0.0 {
            let mut velocity = self.velocity();
            velocity.y -= gravity;
            self.set_velocity(velocity);
        }
    }

    /// Mirrors vanilla `LivingEntity.handleRelativeFrictionAndCalculateMovement()`.
    fn handle_relative_friction_and_calculate_movement(
        &self,
        input: DVec3,
        block_friction: f32,
    ) -> Option<(DVec3, MoveResult)> {
        self.move_relative(self.get_friction_influenced_speed(block_friction), input);
        self.set_velocity(self.handle_on_climbable(self.velocity()));
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        let mut movement = self.velocity();
        if (result.horizontal_collision || self.is_jumping())
            && (self.on_climbable() || self.was_in_powder_snow() && self.can_walk_on_powder_snow())
        {
            movement.y = 0.2;
        }

        Some((movement, result))
    }

    /// Mirrors vanilla `LivingEntity.travelInAir()`.
    fn travel_in_air(&self, input: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        let pos_below = self.block_pos_below_that_affects_movement()?;
        let block_friction = if self.on_ground() {
            world.get_block_state(pos_below).get_block().config.friction
        } else {
            1.0
        };
        let horizontal_friction = block_friction * 0.91;
        let (movement, result) =
            self.handle_relative_friction_and_calculate_movement(input, block_friction)?;
        let movement_y = if let Some(levitation_y) = self.levitation_travel_y_delta(movement.y) {
            movement.y + levitation_y
        } else {
            movement.y - self.get_effective_gravity()
        };

        if self.should_discard_friction() {
            self.set_velocity(DVec3::new(movement.x, movement_y, movement.z));
        } else {
            let vertical_friction = self.air_travel_vertical_friction(horizontal_friction);
            self.set_velocity(DVec3::new(
                movement.x * f64::from(horizontal_friction),
                movement_y * f64::from(vertical_friction),
                movement.z * f64::from(horizontal_friction),
            ));
        }

        Some(result)
    }

    /// Mirrors vanilla `LivingEntity.getFluidFallingAdjustedMovement()`.
    fn get_fluid_falling_adjusted_movement(
        &self,
        base_gravity: f64,
        is_falling: bool,
        movement: DVec3,
    ) -> DVec3 {
        if base_gravity == 0.0 || self.is_sprinting() {
            return movement;
        }

        let y = if is_falling
            && (movement.y - 0.005).abs() >= 0.003
            && (movement.y - base_gravity / 16.0).abs() < 0.003
        {
            -0.003
        } else {
            movement.y - base_gravity / 16.0
        };

        DVec3::new(movement.x, y, movement.z)
    }

    /// Mirrors vanilla `LivingEntity.jumpOutOfFluid()`.
    fn jump_out_of_fluid(&self, old_y: f64) {
        if !self.horizontal_collision() {
            return;
        }

        let movement = self.velocity();
        let target_delta = DVec3::new(
            movement.x,
            movement.y + f64::from(0.6_f32) - self.position().y + old_y,
            movement.z,
        );
        if self.is_free(target_delta) {
            self.set_velocity(DVec3::new(movement.x, f64::from(0.3_f32), movement.z));
        }
    }

    /// Mirrors vanilla `LivingEntity.floatInWaterWhileRidden()`.
    fn float_in_water_while_ridden(&self) {
        if !REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::CAN_FLOAT_WHILE_RIDDEN)
        {
            return;
        }
        if !self.is_vehicle()
            || self.fluid_contact().water_height() <= self.get_fluid_jump_threshold()
        {
            return;
        }

        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    }

    /// Mirrors vanilla `LivingEntity.travelInWater()`.
    fn travel_in_water(
        &self,
        input: DVec3,
        base_gravity: f64,
        is_falling: bool,
        old_y: f64,
    ) -> Option<MoveResult> {
        let mut slow_down = if self.is_sprinting() {
            0.9
        } else {
            self.get_water_slow_down()
        };
        let mut speed = 0.02;
        let mut water_movement_efficiency = self.water_movement_efficiency();
        if !self.on_ground() {
            water_movement_efficiency *= 0.5;
        }

        if water_movement_efficiency > 0.0 {
            slow_down += (0.546_000_06 - slow_down) * water_movement_efficiency;
            speed += (self.get_speed() - speed) * water_movement_efficiency;
        }

        if self.has_dolphins_grace() {
            slow_down = 0.96;
        }

        self.move_relative(speed, input);
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        let mut movement = self.velocity();
        if result.horizontal_collision && self.on_climbable() {
            movement.y = 0.2;
        }

        movement = DVec3::new(
            movement.x * f64::from(slow_down),
            movement.y * f64::from(0.8_f32),
            movement.z * f64::from(slow_down),
        );
        self.set_velocity(self.get_fluid_falling_adjusted_movement(
            base_gravity,
            is_falling,
            movement,
        ));
        self.jump_out_of_fluid(old_y);

        Some(result)
    }

    /// Mirrors vanilla `LivingEntity.travelInLava()`.
    fn travel_in_lava(
        &self,
        input: DVec3,
        base_gravity: f64,
        is_falling: bool,
        old_y: f64,
    ) -> Option<MoveResult> {
        self.move_relative(0.02, input);
        let result = self.move_entity(MoverType::SelfMovement, self.velocity())?;
        if self.fluid_contact().lava_height() <= self.get_fluid_jump_threshold() {
            let movement = self.velocity();
            self.set_velocity(DVec3::new(
                movement.x * 0.5,
                movement.y * f64::from(0.8_f32),
                movement.z * 0.5,
            ));
            self.set_velocity(self.get_fluid_falling_adjusted_movement(
                base_gravity,
                is_falling,
                self.velocity(),
            ));
        } else {
            self.set_velocity(self.velocity() * 0.5);
        }

        if base_gravity != 0.0 {
            self.set_velocity(self.velocity() + DVec3::new(0.0, -base_gravity / 4.0, 0.0));
        }

        self.jump_out_of_fluid(old_y);

        Some(result)
    }

    /// Mirrors vanilla `LivingEntity.travelInFluid()`.
    fn travel_in_fluid(&self, input: DVec3) -> Option<MoveResult> {
        let is_falling = self.velocity().y <= 0.0;
        let old_y = self.position().y;
        let base_gravity = self.get_effective_gravity();
        if self.is_in_water() {
            let result = self.travel_in_water(input, base_gravity, is_falling, old_y);
            self.float_in_water_while_ridden();
            return result;
        }

        self.travel_in_lava(input, base_gravity, is_falling, old_y)
    }

    /// Mirrors the validation part of vanilla `LivingEntity.updateFallFlying()`.
    fn update_fall_flying(&self) {
        self.check_fall_distance_accumulation();
        if self.can_glide() {
            if let Some(free_fall_interval) =
                fall_flying_free_fall_interval(self.fall_flying_ticks())
            {
                if free_fall_interval % 2 == 0 {
                    self.damage_random_glider();
                }
                if let Some(world) = self.level() {
                    world.game_event_at(
                        &vanilla_game_events::ELYTRA_GLIDE,
                        self.position(),
                        &GameEventContext::new(Some(self.as_entity_event_source()), None),
                    );
                }
            }
        } else {
            self.set_fall_flying(false);
        }
    }

    /// Mirrors vanilla `LivingEntity.updateFallFlyingMovement()`.
    fn update_fall_flying_movement(&self, mut movement: DVec3) -> DVec3 {
        let look_angle = self.look_angle();
        let pitch_radians = self.rotation().1.to_radians();
        let look_horizontal_length = horizontal_distance(look_angle);
        let move_horizontal_length = horizontal_distance(movement);
        let gravity = self.get_effective_gravity();
        let lift_force = f64::from(pitch_radians).cos().powi(2);
        movement.y += gravity * (-1.0 + lift_force * 0.75);

        if movement.y < 0.0 && look_horizontal_length > 0.0 {
            let convert = movement.y * -0.1 * lift_force;
            movement += DVec3::new(
                look_angle.x * convert / look_horizontal_length,
                convert,
                look_angle.z * convert / look_horizontal_length,
            );
        }

        if pitch_radians < 0.0 && look_horizontal_length > 0.0 {
            let convert = move_horizontal_length * -f64::from(pitch_radians.sin()) * 0.04;
            movement += DVec3::new(
                -look_angle.x * convert / look_horizontal_length,
                convert * 3.2,
                -look_angle.z * convert / look_horizontal_length,
            );
        }

        if look_horizontal_length > 0.0 {
            movement += DVec3::new(
                (look_angle.x / look_horizontal_length * move_horizontal_length - movement.x) * 0.1,
                0.0,
                (look_angle.z / look_horizontal_length * move_horizontal_length - movement.z) * 0.1,
            );
        }

        DVec3::new(
            movement.x * f64::from(0.99_f32),
            movement.y * f64::from(0.98_f32),
            movement.z * f64::from(0.99_f32),
        )
    }

    /// Mirrors vanilla `LivingEntity.stopFallFlying()`.
    fn stop_fall_flying(&self) {
        self.set_fall_flying(true);
        self.set_fall_flying(false);
    }

    /// Mirrors vanilla `LivingEntity.handleFallFlyingCollisions()`.
    fn handle_fall_flying_collisions(
        &self,
        previous_horizontal_speed: f64,
        new_horizontal_speed: f64,
    ) {
        if !self.horizontal_collision() {
            return;
        }

        let damage = fall_flying_collision_damage(previous_horizontal_speed, new_horizontal_speed);
        if damage <= 0.0 {
            return;
        }

        self.play_sound(self.fall_damage_sound(damage as i32), 1.0, 1.0);
        if let Some(world) = self.level() {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::FLY_INTO_WALL),
                damage,
            );
        }
    }

    /// Mirrors vanilla `LivingEntity.travelFallFlying()`.
    fn travel_fall_flying(&self, input: DVec3) -> Option<MoveResult> {
        if self.on_climbable() {
            let result = self.travel_in_air(input);
            self.stop_fall_flying();
            return result;
        }

        let previous_movement = self.velocity();
        let previous_horizontal_speed = horizontal_distance(previous_movement);
        self.set_velocity(self.update_fall_flying_movement(previous_movement));
        let result = self.move_entity(MoverType::SelfMovement, self.velocity());
        let new_horizontal_speed = horizontal_distance(self.velocity());
        self.handle_fall_flying_collisions(previous_horizontal_speed, new_horizontal_speed);
        result
    }

    /// Default vanilla `LivingEntity.travel()` implementation for overrides.
    fn default_travel(&self, input: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        let fluid_state = get_fluid_state(&world, self.block_position());
        if self.should_travel_in_fluid(fluid_state) {
            return self.travel_in_fluid(input);
        }
        if self.is_fall_flying() {
            return self.travel_fall_flying(input);
        }

        self.travel_in_air(input)
    }

    /// Mirrors vanilla `LivingEntity.travel()`.
    fn travel(&self, input: DVec3) -> Option<MoveResult> {
        self.default_travel(input)
    }

    /// Returns the bed position that makes this living entity sleeping.
    fn sleeping_pos(&self) -> Option<BlockPos> {
        self.living_base().sleeping_pos()
    }

    /// Sets the vanilla living-entity sleeping position.
    fn set_sleeping_pos(&self, bed_position: BlockPos) {
        self.living_base().set_sleeping_pos(bed_position);
    }

    /// Clears the vanilla living-entity sleeping position.
    fn clear_sleeping_pos(&self) {
        self.living_base().clear_sleeping_pos();
    }

    /// Checks if the entity is sleeping.
    fn is_sleeping(&self) -> bool {
        self.sleeping_pos().is_some()
    }

    /// Stops the entity from sleeping.
    fn stop_sleeping(&self) {
        self.clear_sleeping_pos();
    }

    /// Checks if the entity is sprinting.
    fn is_sprinting(&self) -> bool {
        self.living_base().is_sprinting()
    }

    /// Sets whether the entity is sprinting.
    fn set_sprinting(&self, sprinting: bool) {
        self.set_shared_sprinting(sprinting);
        self.living_base().set_sprinting(sprinting);
    }

    /// Gets the entity's cached movement speed.
    fn get_speed(&self) -> f32 {
        self.living_base().speed()
    }

    /// Sets the entity's cached movement speed.
    fn set_speed(&self, speed: f32) {
        self.living_base().set_speed(speed);
    }

    /// Applies vanilla post-impulse movement validation grace.
    fn apply_post_impulse_grace_time(&self, ticks: i32) {
        self.living_base().apply_post_impulse_grace_time(ticks);
    }

    /// Mirrors vanilla `LivingEntity.setIgnoreFallDamageFromCurrentImpulse`.
    fn set_ignore_fall_damage_from_current_impulse(
        &self,
        ignore_fall_damage: bool,
        new_impulse_impact_pos: DVec3,
    ) {
        self.living_base()
            .set_ignore_fall_damage_from_current_impulse(
                ignore_fall_damage,
                new_impulse_impact_pos,
            );
    }

    /// Returns vanilla `LivingEntity.isIgnoringFallDamageFromCurrentImpulse`.
    fn is_ignoring_fall_damage_from_current_impulse(&self) -> bool {
        self.living_base()
            .is_ignoring_fall_damage_from_current_impulse()
    }

    /// Returns vanilla `LivingEntity.currentImpulseImpactPos`.
    fn current_impulse_impact_pos(&self) -> Option<DVec3> {
        self.living_base().current_impulse_impact_pos()
    }

    /// Mirrors vanilla `LivingEntity.tryResetCurrentImpulseContext`.
    fn try_reset_current_impulse_context(&self) {
        self.living_base().try_reset_current_impulse_context();
    }

    /// Mirrors vanilla `LivingEntity.resetCurrentImpulseContext`.
    fn reset_current_impulse_context(&self) {
        self.living_base().reset_current_impulse_context();
    }

    /// Returns whether movement validation is inside post-impulse grace.
    fn is_in_post_impulse_grace_time(&self) -> bool {
        self.living_base().is_in_post_impulse_grace_time()
    }

    /// Decrements post-impulse grace once per living-entity tick.
    fn tick_post_impulse_grace_time(&self) {
        self.living_base().tick_post_impulse_grace_time();
    }

    /// Drains dirty attributes and applies server-side effects.
    fn refresh_dirty_attributes(&self) {
        let dirty = self.attributes().lock().drain_dirty_updates();
        for attr in dirty {
            if attr.key == vanilla_attributes::MAX_HEALTH.key {
                let max = self.get_max_health();
                if self.get_health() > max {
                    self.set_health(max);
                }
            } else if attr.key == vanilla_attributes::MAX_ABSORPTION.key {
                let max = self
                    .attributes()
                    .lock()
                    .get_value(vanilla_attributes::MAX_ABSORPTION)
                    .unwrap_or(0.0) as f32;
                if self.get_absorption_amount() > max {
                    self.set_absorption_amount(max);
                }
            } else if attr.key == vanilla_attributes::SCALE.key {
                self.refresh_dimensions();
            }
            // TODO: WAYPOINT_TRANSMIT_RANGE → waypoint manager
        }
    }
}

fn death_loot_items_with_rng<R: rand::Rng, E: LivingEntity + ?Sized>(
    entity: &E,
    loot_table: LootTableRef,
    world: &World,
    source: &DamageSource,
    killed_by_player: bool,
    rng: &mut R,
) -> Vec<ItemStack> {
    let causing_entity = source
        .causing_entity_id
        .and_then(|entity_id| world.get_entity_by_id(entity_id));
    let direct_entity = source
        .direct_entity_id
        .and_then(|entity_id| world.get_entity_by_id(entity_id));
    let last_damage_player = if killed_by_player {
        entity
            .last_hurt_by_player_uuid()
            .and_then(|uuid| world.get_entity_by_uuid(&uuid))
    } else {
        None
    };

    let position = entity.position();
    let this_entity = living_entity_loot_ref(entity);
    let causing_entity = causing_entity.as_deref().map(entity_loot_ref);
    let direct_entity = direct_entity.as_deref().map(entity_loot_ref);
    let last_damage_player = last_damage_player.as_deref().map(entity_loot_ref);
    let damage_source = DamageSourceInfo {
        damage_type: Some(&source.damage_type.key),
        tags: &[],
        is_direct: source.is_direct(),
    };

    let mut context = LootContext::new(rng)
        .with_origin(position.x, position.y, position.z)
        .with_game_time(world.game_time())
        .with_killed_by_player(killed_by_player)
        .with_this_entity(this_entity)
        .with_damage_source(damage_source);
    if let Some(entity) = causing_entity {
        context = context.with_killer_entity(entity);
    }
    if let Some(entity) = direct_entity {
        context = context.with_direct_killer_entity(entity);
    }
    if let Some(entity) = last_damage_player {
        context = context.with_last_damage_player(entity);
    }

    loot_table.get_random_items(&mut context)
}

fn living_entity_loot_ref<E: LivingEntity + ?Sized>(entity: &E) -> EntityRef<'_> {
    EntityRef {
        entity_type: Some(&entity.entity_type().key),
        flags: EntityRefFlags {
            is_on_fire: entity.is_on_fire(),
            is_sneaking: entity.is_crouching(),
            is_sprinting: entity.is_sprinting(),
            is_swimming: entity.is_swimming(),
            is_baby: entity.is_baby(),
        },
        // TODO: Include equipment and custom name once loot contexts can snapshot entity data.
        equipment: None,
        custom_name: None,
    }
}
