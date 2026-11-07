//! Living damage, death and loot.

use super::*;

impl Living<'_> {
    /// Returns whether this entity may attack `target`: never a player on
    /// peaceful difficulty, and only visible, non-invulnerable targets otherwise.
    #[must_use]
    pub fn can_attack(&self, target: &Living<'_>) -> bool {
        if target.entity_type() == &vanilla_entities::PLAYER
            && self
                .level()
                .is_some_and(|world| world.difficulty() == Difficulty::Peaceful)
        {
            return false;
        }

        target.can_be_seen_as_enemy()
    }

    /// Returns the last damage source, unless it's older than the timeout window, with
    /// [`RecentDamageSource`] retention semantics.
    #[must_use]
    pub fn last_damage_source(&self) -> Option<RecentDamageSource> {
        self.base().damage_history().last_damage_source()
    }

    /// Records a successful hit using the victim's own level clock.
    ///
    /// # Panics
    ///
    /// Panics if the entity has no level or the server has no damage history.
    pub fn record_last_damage_source(&self, source: &DamageSource) {
        let Some(world) = self.level() else {
            panic!("recording living damage requires the victim's level");
        };
        let Some(history) = world.damage_history.upgrade() else {
            panic!("recording living damage requires a live server damage history");
        };
        history.record(self.base(), source, &world.game_time);
    }

    /// Records the player that last hurt this entity.
    pub fn set_last_hurt_by_player(&self, player_uuid: Uuid, time_to_remember: i32) {
        self.living_base()
            .set_last_hurt_by_player(player_uuid, time_to_remember);
    }

    /// Returns the remaining hurt-cooldown ticks (vanilla `Entity.invulnerableTime`).
    #[must_use]
    pub fn invulnerable_time(&self) -> i32 {
        self.living_base().invulnerable_time()
    }

    /// Sets the remaining hurt-cooldown ticks.
    pub fn set_invulnerable_time(&self, invulnerable_time: i32) {
        self.living_base().set_invulnerable_time(invulnerable_time);
    }

    /// Sets the remaining memory time without touching the remembered player.
    pub fn set_last_hurt_by_player_memory_time(&self, time_to_remember: i32) {
        self.living_base()
            .set_last_hurt_by_player_memory_time(time_to_remember);
    }

    /// Returns ticks remaining that this entity will remember the player who last hurt it.
    #[must_use]
    pub fn last_hurt_by_player_memory_time(&self) -> i32 {
        self.living_base().last_hurt_by_player_memory_time()
    }

    /// Returns vanilla `LivingEntity.lastHurtByPlayer`, if still remembered.
    #[must_use]
    pub fn last_hurt_by_player_uuid(&self) -> Option<Uuid> {
        self.living_base().last_hurt_by_player_uuid()
    }

    /// Returns the mob that last hurt this entity, if still resolvable.
    #[must_use]
    pub fn last_hurt_by_mob(&self) -> Option<SharedEntity> {
        self.living_base().last_hurt_by_mob()
    }

    /// Returns the tick timestamp this entity was last hurt by a mob.
    #[must_use]
    pub fn last_hurt_by_mob_timestamp(&self) -> i32 {
        self.living_base().last_hurt_by_mob_timestamp()
    }

    /// Records the mob that last hurt this entity.
    pub fn set_last_hurt_by_mob(&self, target: Option<&SharedEntity>) {
        self.living_base()
            .set_last_hurt_by_mob(target, self.tick_count());
    }

    /// Returns the mob this entity last hurt, if still resolvable.
    #[must_use]
    pub fn last_hurt_mob(&self) -> Option<SharedEntity> {
        self.living_base().last_hurt_mob()
    }

    /// Returns the tick timestamp this entity last hurt a mob.
    #[must_use]
    pub fn last_hurt_mob_timestamp(&self) -> i32 {
        self.living_base().last_hurt_mob_timestamp()
    }

    /// Records the mob this entity last hurt.
    pub fn set_last_hurt_mob(&self, target: Option<&SharedEntity>) {
        self.living_base()
            .set_last_hurt_mob(target, self.tick_count());
    }

    /// Records the causing entity as this entity's last-hurt-by mob, skipping
    /// damage types that shouldn't provoke anger (e.g. `no_anger`, or wind
    /// charges when this mob is immune to their anger).
    pub fn resolve_mob_responsible_for_damage(&self, source: &DamageSource) {
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

        let Some(entity) = source.causing_entity() else {
            return;
        };
        if entity.is_living_entity() {
            self.set_last_hurt_by_mob(Some(entity));
        }
    }

    /// Records the causing entity as this entity's last-hurt-by player, if
    /// the causing entity is a player.
    pub fn resolve_player_responsible_for_damage(&self, source: &DamageSource) {
        let Some(entity) = source.causing_entity() else {
            return;
        };
        if entity.as_player().is_some() {
            self.living_base()
                .set_last_hurt_by_player_entity(entity, 100);
        }
    }

    /// Returns whether this entity has an unobstructed line of sight to target's eyes.
    pub fn has_line_of_sight(&self, target: &dyn Entity) -> bool {
        self.has_line_of_sight_with(
            target,
            ClipBlockShape::Collider,
            ClipFluid::None,
            target.eye_y(),
        )
    }

    /// Returns vanilla line-of-sight with explicit clip options.
    pub fn has_line_of_sight_with(
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
        let start = DVec3::new(position.x, self.eye_y(), position.z);
        let end = DVec3::new(target_position.x, target_eye_y, target_position.z);
        if start.distance_squared(end) > 128.0 * 128.0 {
            return false;
        }

        world.clip(start, end, block_shape, fluid).is_miss()
    }

    /// Returns vanilla base living-entity invulnerability.
    #[must_use]
    pub fn default_is_invulnerable_to(&self, source: &DamageSource) -> bool {
        self.is_invulnerable_to_base(source)
    }

    /// Returns whether this living entity ignores a damage source.
    pub fn is_invulnerable_to_default(&self, _world: &World, source: &DamageSource) -> bool {
        self.default_is_invulnerable_to(source)
            || enchantment_helper::is_immune_to_damage(self, source)
    }

    /// Main vanilla living-entity damage entry point.
    ///
    /// `world` is the `ServerLevel` supplied by the vanilla caller. It may
    /// intentionally differ from the entity's attached world.
    pub fn hurt_server(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.default_hurt_server(world, source, amount)
    }

    /// Runs the base `LivingEntity.hurtServer` body.
    ///
    /// Overrides of [`Self::hurt_server`] call this where vanilla calls
    /// `super.hurtServer`; the Ender Dragon routes its per-part damage scaling
    /// through here.
    pub fn default_hurt_server(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
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
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::DAMAGES_HELMET) {
            let mut wears_helmet = false;
            self.with_equipment_slot(EquipmentSlot::Head, &mut |item| {
                wears_helmet = !item.is_empty();
            });
            if wears_helmet {
                self.hurt_helmet(source, damage);
                damage *= 0.75;
            }
        }
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
        if took_full_damage {
            self.living_base().begin_hurt_animation();
        }
        self.resolve_mob_responsible_for_damage(source);
        self.resolve_player_responsible_for_damage(source);

        if took_full_damage {
            self.broadcast_damage_event(world, source);
            if !source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_IMPACT) {
                self.mark_hurt();
            }
            self.apply_damage_knockback(source);
        }

        if self.is_dead_or_dying() {
            if !self.check_totem_death_protection(source) {
                if took_full_damage {
                    self.play_death_sound();
                }
                self.die(source);
            }
        } else if took_full_damage {
            self.play_hurt_sound(source);
        }
        // TODO: Play secondary hurt sounds once equipment effects expose them.

        self.record_last_damage_source(source);

        true
    }

    /// Hook before applying damage after vanilla reductions.
    pub fn before_actually_hurt(&self, _source: &DamageSource, _amount: f32) {
        if let Some(animal) = Animal::of(self.as_entity_event_source()) {
            animal.reset_love();
        }
    }

    /// Damages equipment that participates in vanilla armor absorption.
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn hurt_armor_default(&self, _source: &DamageSource, _damage: f32) {}

    /// Damages a worn helmet; base living entities do not.
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn hurt_helmet_default(&self, _source: &DamageSource, _damage: f32) {}

    /// Damages equipped items in the given slots that are eligible to absorb this damage source.
    pub fn do_hurt_equipment(&self, source: &DamageSource, damage: f32, slots: &[EquipmentSlot]) {
        if damage <= 0.0 {
            return;
        }

        let durability_damage = (damage / 4.0).max(1.0) as i32;
        for &slot in slots {
            let mut item_broke = false;
            let mut item_ref = &*vanilla_items::AIR;
            self.with_equipment_slot_mut(slot, &mut |item| {
                item_ref = item.item;
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
                self.on_equipped_item_broken(item_ref, slot);
            }
        }
    }

    /// Reduces damage by the entity's armor value and toughness, unless the source bypasses armor.
    #[must_use]
    pub fn damage_after_armor_absorb(&self, source: &DamageSource, mut damage: f32) -> f32 {
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
                self.armor_value() as f32,
                armor_toughness,
            );
        }
        damage
    }

    /// Reduces damage based on the resistance effect, unless the source bypasses it or effects entirely.
    #[must_use]
    pub fn damage_after_magic_absorb(&self, source: &DamageSource, mut damage: f32) -> f32 {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_EFFECTS) {
            return damage;
        }

        if !source.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_RESISTANCE)
            && let Some(resistance) = self.mob_effect(vanilla_mob_effects::RESISTANCE)
        {
            let absorb_value = (resistance.amplifier() + 1) * 5;
            let absorb = 25 - absorb_value;
            let old_damage = damage;
            damage = (damage * absorb as f32 / 25.0).max(0.0);
            let damage_resisted = old_damage - damage;
            if (0.0..f32::MAX).contains(&damage_resisted) {
                let stats_to_award = (damage_resisted * 10.0).round() as i32;
                if let Some(player) = self.as_player() {
                    player.award_custom_stat_with_count(
                        &vanilla_custom_stats::DAMAGE_RESISTED,
                        stats_to_award,
                    );
                } else if let Some(damage_causer) = source.causing_entity()
                    && let Some(player) = damage_causer.as_player()
                {
                    player.award_custom_stat_with_count(
                        &vanilla_custom_stats::DAMAGE_DEALT_RESISTED,
                        stats_to_award,
                    );
                }
            }
        }

        if damage <= 0.0 {
            return 0.0;
        }
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_ENCHANTMENTS) {
            return damage;
        }

        let enchantment_armor = enchantment_helper::get_damage_protection(self, source);
        if enchantment_armor > 0.0 {
            damage = combat_rules::damage_after_magic_absorb(damage, enchantment_armor);
        }
        damage
    }

    /// Applies damage after vanilla reductions.
    pub fn actually_hurt_default(&self, world: &World, source: &DamageSource, amount: f32) {
        if self.is_invulnerable_to(world, source) {
            return;
        }

        let damage = self.damage_after_armor_absorb(source, amount);
        let damage = self.damage_after_magic_absorb(source, damage);
        let original_damage = damage;
        let damage = (damage - self.absorption_amount()).max(0.0);
        self.set_absorption_amount(self.absorption_amount() - (original_damage - damage));

        let absorbed_damage = original_damage - damage;
        if (0.0..f32::MAX).contains(&absorbed_damage)
            && let Some(damage_causer) = source.causing_entity()
            && let Some(player) = damage_causer.as_player()
        {
            player.award_custom_stat_with_count(
                &vanilla_custom_stats::DAMAGE_DEALT_ABSORBED,
                (absorbed_damage * 10.0).round() as i32,
            );
        }

        if damage != 0.0 {
            self.set_health(self.health() - damage);
            self.set_absorption_amount(self.absorption_amount() - damage);
            self.game_event(&vanilla_game_events::ENTITY_DAMAGE);
        }
    }

    /// Applies vanilla hurt knockback for a damage source.
    pub fn apply_damage_knockback(&self, source: &DamageSource) {
        if source.is(&vanilla_damage_type_tags::DamageTypeTag::NO_KNOCKBACK) {
            return;
        }

        let (xd, zd) = self.damage_knockback_direction(source);
        self.knockback(DAMAGE_KNOCKBACK_POWER, xd, zd);
        self.indicate_damage(xd, zd);
    }

    /// Returns the horizontal direction used by vanilla damage knockback.
    #[must_use]
    pub fn damage_knockback_direction(&self, source: &DamageSource) -> (f64, f64) {
        if let Some(direct_entity) = source.direct_entity()
            && let Some(projectile) = direct_entity.as_projectile()
            && let Some(hurt_entity) = self.as_living_entity()
        {
            let (xd, zd) =
                projectile.calculate_horizontal_hurt_knockback_direction(&hurt_entity, source);
            return (-xd, -zd);
        }

        let Some(source_position) = source.source_position() else {
            return (0.0, 0.0);
        };

        let position = self.position();
        (
            source_position.x - position.x,
            source_position.z - position.z,
        )
    }

    /// Applies knockback velocity in the direction (`xd`, `zd`), scaled by
    /// `power` and reduced by knockback resistance.
    pub fn knockback(&self, mut power: f64, mut xd: f64, mut zd: f64) {
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
    #[must_use]
    pub fn knockback_resistance(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::KNOCKBACK_RESISTANCE)
    }

    /// Applies a client-visible damage knockback indication in the given direction.
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn indicate_damage_default(&self, _xd: f64, _zd: f64) {}

    /// Returns the chunk used for vanilla nearby hurt broadcasts.
    #[must_use]
    pub fn hurt_broadcast_chunk_default(&self) -> ChunkPos {
        ChunkPos::from_entity_pos(self.position())
    }

    /// Broadcasts vanilla damage-event metadata near this entity.
    pub fn broadcast_damage_event(&self, world: &World, source: &DamageSource) {
        world.broadcast_to_nearby(
            self.hurt_broadcast_chunk(),
            CDamageEvent {
                entity_id: self.id(),
                source_type_id: source.damage_type.id() as i32,
                source_cause_id: source
                    .causing_entity()
                    .map_or(0, |entity| entity.id().wrapping_add(1)),
                source_direct_id: source
                    .direct_entity()
                    .map_or(0, |entity| entity.id().wrapping_add(1)),
                source_position: source.source_position_raw(),
            },
            None,
        );
    }

    /// Uses a held death-protection item to survive lethal damage. Returns whether one was used.
    #[must_use]
    pub fn check_totem_death_protection(&self, source: &DamageSource) -> bool {
        if source.bypasses_invulnerability() {
            return false;
        }

        let mut used = None;
        for slot in [EquipmentSlot::MainHand, EquipmentSlot::OffHand] {
            self.with_equipment_slot_mut(slot, &mut |stack| {
                if let Some(protection) = stack.get(DEATH_PROTECTION).cloned() {
                    used = Some((protection, stack.clone()));
                    stack.shrink(1);
                }
            });
            if used.is_some() {
                break;
            }
        }
        let Some((protection, protection_item)) = used else {
            return false;
        };

        if let Some(player) = self.as_player() {
            player.award_stat(&vanilla_stat_types::ITEM_USED, protection_item.item());
            // TODO: trigger the `used_totem` advancement criterion once advancements exist.
            if protection_item
                .get(USE_EFFECTS)
                .is_some_and(|effects| effects.interact_vibrations)
            {
                self.game_event(&vanilla_game_events::ITEM_INTERACT_FINISH);
            }
        }

        self.set_health(1.0);
        if let (Some(world), Some(entity)) = (self.level(), self.as_living_entity()) {
            for effect in protection.death_effects() {
                apply_consume_effect(effect, &world, &entity);
            }
        }
        self.broadcast_entity_event(EntityStatus::ProtectedFromDeath);
        true
    }

    /// Processes vanilla living death side effects.
    pub fn die_default(&self, source: &DamageSource) {
        if self.is_removed() {
            return;
        }
        if !self.living_base().mark_death_processed() {
            return;
        }

        self.handle_killing_blow();

        // Can't directly use &self for &Living<'_>, as the compiler doesn't know if it's Sized.
        // Using a function meant for getting &Living<'_> directly works well here.
        if let Some(world) = self.level()
            && let Some(self_entity) = self.as_living_entity()
        {
            let source_entity = source.causing_entity();
            if source_entity.is_none_or(|entity| entity.killed_entity(&world, &self_entity, source))
            {
                self.game_event(&vanilla_game_events::ENTITY_DIE);
                self.drop_all_death_loot(source);
                // TODO: Create wither rose for killer
            }
        }

        self.broadcast_entity_event(EntityStatus::Death);
        self.set_pose(EntityPose::Dying);
    }

    /// Runs vanilla `LivingEntity.handleKillingBlow`.
    ///
    /// Vanilla's body is `this.dead = true`, which [`Self::die`] has already done
    /// atomically by the time this runs. The hook exists for the subclasses that
    /// replace it: bosses playing a death animation clamp their health and enter a
    /// dying state here instead of falling over.
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn handle_killing_blow_default(&self) {}

    /// Returns vanilla `LivingEntity.shouldDropLoot`.
    /// Returns whether this entity should drop loot on death: not a baby,
    /// and the mob-drops game rule allows it.
    pub fn should_drop_loot(&self, world: &World) -> bool {
        !self.is_baby() && world.get_game_rule(&MOB_DROPS)
    }

    /// Returns whether this entity should drop experience on death: not a baby.
    #[must_use]
    pub fn should_drop_experience(&self) -> bool {
        !self.is_baby()
    }

    /// Returns whether this entity always drops experience regardless of the
    /// player-kill requirement; false by default.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn is_always_experience_dropper_default(&self) -> bool {
        false
    }

    /// Marks this entity to skip dropping experience on death.
    pub fn skip_drop_experience(&self) {
        self.living_base().skip_drop_experience();
    }

    /// Returns whether this entity has been marked to skip dropping experience.
    #[must_use]
    pub fn was_experience_consumed(&self) -> bool {
        self.living_base().was_experience_consumed()
    }

    /// Returns the base experience reward for killing this entity, using the
    /// animal-specific reward when applicable.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn base_experience_reward_default(&self) -> i32 {
        0
    }

    /// Returns the experience reward for this entity's death; currently
    /// equal to the base reward (see TODO).
    pub fn experience_reward(&self, _world: &World, _killer_entity: Option<&dyn Entity>) -> i32 {
        // TODO: Apply EnchantmentHelper.processMobExperience once enchantment
        // value-effect hooks can receive the killer/living-entity context.
        self.base_experience_reward()
    }

    /// Runs the currently implemented subset of vanilla `LivingEntity.dropAllDeathLoot`.
    pub fn drop_all_death_loot(&self, source: &DamageSource) {
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
        self.drop_experience(&world, source.causing_entity().map(AsRef::as_ref));
        // TODO: Drop non-mob equipment overrides once those foundations exist.
    }

    /// Awards experience for this entity's death when eligible, spawning an experience orb.
    pub fn drop_experience(&self, world: &Arc<World>, killer_entity: Option<&dyn Entity>) {
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

        let reward = self.experience_reward(world, killer_entity);
        if reward > 0 {
            ExperienceOrbEntity::award(world, self.position(), reward);
        }
    }

    /// Returns this entity's death loot table: a mob's custom override if
    /// set, otherwise the default table for its entity type.
    #[must_use]
    pub fn death_loot_table(&self) -> Option<LootTableRef> {
        if let Some(mob) = self.as_mob()
            && mob.has_custom_death_loot_table()
        {
            return mob.custom_death_loot_table();
        }

        let entity_type = self.entity_type();
        let loot_key = Identifier::vanilla(format!("entities/{}", entity_type.key.path));
        REGISTRY.loot_tables.by_key(&loot_key)
    }

    /// Returns the loot-table seed for this entity's death loot, if it's a mob.
    #[must_use]
    pub fn death_loot_table_seed(&self) -> i64 {
        self.as_mob().map_or(0, |mob| mob.death_loot_table_seed())
    }

    /// Rolls and spawns this entity's death-loot drops from its loot table,
    /// seeded deterministically when a loot-table seed is set.
    pub fn drop_from_loot_table(&self, source: &DamageSource, killed_by_player: bool) {
        let Some(world) = self.level() else {
            return;
        };
        let has_custom_death_loot_table = self
            .as_mob()
            .is_some_and(|mob| mob.has_custom_death_loot_table());
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
    #[expect(
        clippy::unused_self,
        reason = "vanilla's base body; entity types override it through `LivingProps`"
    )]
    pub const fn drop_custom_death_loot_default(
        &self,
        _source: &DamageSource,
        _killed_by_player: bool,
    ) {
    }

    /// Ticks the vanilla living death animation and removes the entity at completion.
    pub fn tick_death(&self) {
        let death_time = self.living_base().increment_death_time();
        if death_time >= DEATH_DURATION && !self.is_removed() {
            self.broadcast_entity_event(EntityStatus::Poof);
            self.set_removed(RemovalReason::Killed);
        }
    }

    /// Gets the absorption amount (extra health from effects like absorption).
    #[must_use]
    pub fn absorption_amount_default(&self) -> f32 {
        self.living_base().absorption_amount()
    }

    /// Sets the absorption amount.
    pub fn set_absorption_amount_default(&self, amount: f32) {
        self.living_base().set_absorption_amount(amount);
    }

    /// `LivingEntity.dropFromShearingLootTable` without the drop: rolls
    /// `loot_table` with the shearing context (origin, this entity and `tool`).
    #[must_use]
    pub fn shearing_loot(&self, loot_table: LootTableRef, tool: &ItemStack) -> Vec<ItemStack> {
        let position = self.position();
        let mut rng = rand::rng();
        let equipment = living_loot_equipment(self);
        let mut context = LootContext::new(&mut rng)
            .with_origin(position.x, position.y, position.z)
            .with_this_entity(entity_loot_ref(self.entity(), Some(&equipment)))
            .with_tool(tool);
        if let Some(world) = self.level() {
            context = context.with_game_time(world.game_time());
        }
        loot_table.get_random_items(&mut context)
    }

    /// `LivingEntity.dropFromGiftLootTable`: rolls `loot_table` with the gift
    /// context (origin and this entity) and drops each item here. Returns
    /// whether anything dropped.
    #[must_use]
    pub fn drop_from_gift_loot_table(&self, loot_table: LootTableRef) -> bool {
        let position = self.position();
        let mut rng = rand::rng();
        let equipment = living_loot_equipment(self);
        let mut context = LootContext::new(&mut rng)
            .with_origin(position.x, position.y, position.z)
            .with_this_entity(entity_loot_ref(self.entity(), Some(&equipment)));

        let items = loot_table.get_random_items(&mut context);
        let dropped_any = !items.is_empty();
        for item in items {
            self.spawn_at_location(item, 0.0);
        }
        dropped_any
    }

    /// Gets the entity's armor value from the attribute system.
    #[must_use]
    pub fn armor_value(&self) -> i32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::ARMOR)
            .unwrap_or(0.0) as i32
    }
}

fn death_loot_items_with_rng<R: rand::Rng>(
    entity: &Living<'_>,
    loot_table: LootTableRef,
    world: &World,
    source: &DamageSource,
    killed_by_player: bool,
    rng: &mut R,
) -> Vec<ItemStack> {
    let causing_entity = source.causing_entity().map(AsRef::as_ref);
    let direct_entity = source.direct_entity().map(AsRef::as_ref);
    let last_damage_player = if killed_by_player {
        entity.living_base().last_hurt_by_player(world)
    } else {
        None
    };

    let position = entity.position();
    let this_equipment = living_loot_equipment(entity);
    let causing_equipment = causing_entity.and_then(loot_equipment);
    let direct_equipment = direct_entity.and_then(loot_equipment);
    let last_damage_player_equipment = last_damage_player.as_deref().and_then(loot_equipment);
    let this_entity = entity_loot_ref(entity.as_entity_event_source(), Some(&this_equipment));
    let causing_entity =
        causing_entity.map(|causing| entity_loot_ref(causing, causing_equipment.as_ref()));
    let direct_entity =
        direct_entity.map(|direct| entity_loot_ref(direct, direct_equipment.as_ref()));
    let last_damage_player = last_damage_player
        .as_deref()
        .map(|player| entity_loot_ref(player, last_damage_player_equipment.as_ref()));
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
