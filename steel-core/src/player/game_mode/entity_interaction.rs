use super::{
    ATTACK_RANGE_BUFFER, CSetEntityMotion, ClipBlockShape, ClipFluid, DVec3, DamageSource,
    DamageType, ENTITY_INTERACTION_RANGE_BUFFER, EnchantmentDamageContext,
    EnchantmentPostAttackContext, Entity, EntityTypeRef, GameType, ITEM_BEHAVIORS, InteractionHand,
    InteractionResult, InventoryAccess, ItemStack, LivingEntity, PiercingWeapon, Player, SAttack,
    SInteract, SharedEntity, SoundEventHolder, SoundEventRef, TextComponent, World, WorldAabb,
    enchantment_helper, piercing_ray_hit_t, translations, vanilla_attributes, vanilla_damage_types,
    vanilla_entities,
};

const fn sound_holder_ref(holder: &SoundEventHolder) -> Option<SoundEventRef> {
    match holder {
        SoundEventHolder::Registry(sound) => Some(*sound),
        SoundEventHolder::Direct { .. } => {
            // TODO: Support direct sound holders when entity sound playback can send them.
            None
        }
    }
}
impl Player {
    fn invalid_entity_attacked_message() -> TextComponent {
        translations::MULTIPLAYER_DISCONNECT_INVALID_ENTITY_ATTACKED
            .msg()
            .component()
    }

    fn eye_position(&self) -> DVec3 {
        let position = self.position();
        DVec3::new(position.x, self.get_eye_y(), position.z)
    }

    fn damage_source_for_attack_type(&self, damage_type: &'static DamageType) -> DamageSource {
        DamageSource::environment(damage_type)
            .with_causing_entity(self.id())
            .with_direct_entity(self.id())
            .with_source_position(self.position())
    }

    fn attack_damage_source(&self, attacking_item: &ItemStack) -> DamageSource {
        if let Some(damage_type) = attacking_item.get_damage_type() {
            return self.damage_source_for_attack_type(damage_type);
        }
        if let Some(source) = ITEM_BEHAVIORS
            .get_behavior(attacking_item.item())
            .get_item_damage_source(self)
        {
            return source;
        }
        self.damage_source_for_attack_type(&vanilla_damage_types::PLAYER_ATTACK)
    }

    /// Ticks vanilla attack-strength recovery and resets it on main-hand item changes.
    pub(in crate::player) fn tick_attack_strength(&self) {
        self.tick_state.lock().advance_attack_strength_ticker();

        let main_hand_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };

        let mut last_item = self.last_item_in_main_hand.lock();
        if ItemStack::matches(&last_item, &main_hand_item) {
            return;
        }

        if !ItemStack::is_same_item(&last_item, &main_hand_item) {
            self.reset_attack_strength_ticker();
        }

        *last_item = main_hand_item;
    }

    fn reset_attack_strength_ticker(&self) {
        self.tick_state.lock().reset_attack_strength_ticker();
    }

    fn current_item_attack_strength_delay(&self) -> f32 {
        let attack_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::ATTACK_SPEED);
        Self::attack_strength_delay_from_speed(attack_speed)
    }

    fn attack_strength_delay_from_speed(attack_speed: f64) -> f32 {
        (1.0 / attack_speed * 20.0) as f32
    }

    /// Returns vanilla `Player.getAttackStrengthScale`.
    #[must_use]
    pub fn attack_strength_scale(&self, partial_tick: f32) -> f32 {
        let attack_strength_delay = self.current_item_attack_strength_delay();
        self.attack_strength_scale_for_delay(partial_tick, attack_strength_delay)
    }

    fn attack_strength_scale_for_delay(
        &self,
        partial_tick: f32,
        attack_strength_delay: f32,
    ) -> f32 {
        let ticker = self.tick_state.lock().attack_strength_ticker() as f32;
        ((ticker + partial_tick) / attack_strength_delay).clamp(0.0, 1.0)
    }

    fn base_damage_scale_factor(attack_strength_scale: f32) -> f32 {
        0.2 + attack_strength_scale * attack_strength_scale * 0.8
    }

    fn get_knockback(
        attack_knockback: f64,
        weapon: &ItemStack,
        enchantment_context: &EnchantmentDamageContext<'_>,
    ) -> f64 {
        let modified = enchantment_helper::modify_knockback(
            weapon,
            enchantment_context,
            attack_knockback as f32,
        );
        f64::from(modified) / 2.0
    }

    fn cause_extra_knockback(
        &self,
        entity: &dyn Entity,
        knockback_amount: f64,
        old_movement: DVec3,
    ) {
        if knockback_amount > 0.0 {
            let yaw_radians = self.rotation().0.to_radians();
            let yaw_sin = f64::from(yaw_radians.sin());
            let yaw_cos = f64::from(yaw_radians.cos());
            if let Some(living_target) = entity.as_living_entity() {
                living_target.knockback(knockback_amount, yaw_sin, -yaw_cos);
            } else {
                entity.push_impulse(DVec3::new(
                    -yaw_sin * knockback_amount,
                    0.1,
                    yaw_cos * knockback_amount,
                ));
            }

            let velocity = self.velocity();
            self.set_velocity(DVec3::new(velocity.x * 0.6, velocity.y, velocity.z * 0.6));
            self.set_sprinting(false);
        }

        if entity.entity_type() == &vanilla_entities::PLAYER
            && entity.hurt_marked()
            && let Some(player) = self.get_world().players.get_by_entity_id(entity.id())
        {
            let velocity = entity.velocity();
            player.send_packet(CSetEntityMotion::new(entity.id(), velocity));
            entity.clear_hurt_mark();
            entity.set_velocity(old_movement);
        }
    }

    fn entity_interaction_range(&self) -> f64 {
        self.attributes()
            .lock()
            .required_value(vanilla_attributes::ENTITY_INTERACTION_RANGE)
    }

    /// Returns true if the target box is within the player's attack range for `item_stack`.
    #[must_use]
    pub fn is_within_attack_range_with_buffer(
        &self,
        item_stack: &ItemStack,
        aabb: WorldAabb,
        buffer: f64,
    ) -> bool {
        let distance = aabb.distance_to_sqr(self.eye_position()).sqrt();
        let (min_reach, max_reach, hitbox_margin) =
            if let Some(attack_range) = item_stack.get_attack_range() {
                if self.game_mode() == GameType::Creative {
                    (
                        attack_range.min_creative_reach,
                        attack_range.max_creative_reach,
                        attack_range.hitbox_margin,
                    )
                } else {
                    (
                        attack_range.min_reach,
                        attack_range.max_reach,
                        attack_range.hitbox_margin,
                    )
                }
            } else {
                (0.0, self.entity_interaction_range() as f32, 0.0)
            };
        let min_reach = f64::from(min_reach) - f64::from(hitbox_margin) - buffer;
        let max_reach = f64::from(max_reach) + f64::from(hitbox_margin) + buffer;
        distance >= min_reach && distance <= max_reach
    }

    /// Returns true if the target box is within the player's entity interaction range.
    #[must_use]
    pub fn is_within_entity_interaction_range_with_buffer(
        &self,
        aabb: WorldAabb,
        buffer: f64,
    ) -> bool {
        let max_range = self.entity_interaction_range() + buffer;
        aabb.distance_to_sqr(self.eye_position()) <= max_range * max_range
    }

    fn attack_range_for_item(&self, item_stack: &ItemStack) -> (f64, f64, f64) {
        let Some(attack_range) = item_stack.get_attack_range() else {
            return (0.0, self.entity_interaction_range(), 0.0);
        };

        let (min_reach, max_reach) = if self.game_mode() == GameType::Creative {
            (
                attack_range.min_creative_reach,
                attack_range.max_creative_reach,
            )
        } else {
            (attack_range.min_reach, attack_range.max_reach)
        };
        (
            f64::from(min_reach),
            f64::from(max_reach),
            f64::from(attack_range.hitbox_margin),
        )
    }

    fn piercing_hit_entities(&self, item_stack: &ItemStack, world: &World) -> Vec<SharedEntity> {
        let look = self.look_angle();
        if look.length_squared() <= f64::EPSILON {
            return Vec::new();
        }

        let (min_reach, max_reach, hitbox_margin) = self.attack_range_for_item(item_stack);
        let eye_position = self.eye_position();
        let from = eye_position + look * min_reach;
        let movement_extension = self.known_movement().dot(look).max(0.0);
        let mut to = eye_position + look * (max_reach + movement_extension);

        let block_hit = world.clip(eye_position, to, ClipBlockShape::Collider, ClipFluid::None);
        if !block_hit.is_miss() {
            to = block_hit.location;
            if eye_position.distance_squared(to) < eye_position.distance_squared(from) {
                return Vec::new();
            }
        }

        let search_area = WorldAabb::new(from.x, from.y, from.z, from.x, from.y, from.z)
            .inflate_xyz(hitbox_margin, hitbox_margin, hitbox_margin)
            .expand_towards(to - from)
            .inflate(1.0);
        let mut hits = world
            .get_entities_in_aabb_matching(&search_area, |entity| {
                self.can_piercing_hit_entity(entity)
            })
            .into_iter()
            .filter_map(|entity| {
                piercing_ray_hit_t(world, entity.bounding_box(), from, to, hitbox_margin)
                    .map(|hit_t| (hit_t, entity))
            })
            .collect::<Vec<_>>();
        hits.sort_by(|(left, _), (right, _)| left.total_cmp(right));
        hits.into_iter().map(|(_, entity)| entity).collect()
    }

    fn can_piercing_hit_entity(&self, target: &dyn Entity) -> bool {
        target.id() != self.id()
            && !target.is_invulnerable()
            && target.is_alive()
            && target.can_be_hit_by_projectile()
            && !self.is_passenger_of_same_vehicle(target)
    }

    pub(super) fn piercing_attack(&self, item_stack: &ItemStack, piercing_weapon: &PiercingWeapon) {
        let world = self.get_world();
        let base_damage = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::ATTACK_DAMAGE) as f32;
        let mut hit_something = false;
        for target in self.piercing_hit_entities(item_stack, &world) {
            hit_something |= self.stab_attack(
                &target,
                base_damage,
                true,
                piercing_weapon.deals_knockback,
                piercing_weapon.dismounts,
            );
        }

        self.reset_attack_strength_ticker();
        enchantment_helper::do_post_piercing_attack_effects(&world, self);
        if hit_something {
            self.play_sound_holder(piercing_weapon.hit_sound.as_ref());
        }
        self.play_sound_holder(piercing_weapon.sound.as_ref());
        self.swing(InteractionHand::MainHand, false);
    }

    fn stab_attack(
        &self,
        target: &SharedEntity,
        base_damage: f32,
        deals_damage: bool,
        deals_knockback: bool,
        dismounts: bool,
    ) -> bool {
        let entity = target.as_ref();
        if self.cannot_attack(entity) {
            return false;
        }

        let attacking_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };
        let damage_source = self.attack_damage_source(&attacking_item);
        let enchantment_context = EnchantmentDamageContext::new(
            entity.entity_type(),
            Some(self.entity_type()),
            Some(self.entity_type()),
            &damage_source,
        );
        let enchanted_damage =
            enchantment_helper::modify_damage(&attacking_item, &enchantment_context, base_damage);
        let attack_strength_scale = self.attack_strength_scale(0.5);
        let magic_boost = attack_strength_scale * (enchanted_damage - base_damage);
        let base_damage = base_damage * Self::base_damage_scale_factor(attack_strength_scale);
        let damage = base_damage + magic_boost;
        let old_movement = entity.velocity();
        let mut affected = deals_knockback;
        let damage_dealt = deals_damage
            && entity
                .level()
                .is_some_and(|world| entity.hurt(&world, &damage_source, damage));
        affected |= damage_dealt;
        if deals_knockback {
            self.cause_extra_knockback(
                entity,
                0.4 + Self::get_knockback(0.0, &attacking_item, &enchantment_context),
                old_movement,
            );
        }
        if dismounts && entity.is_passenger() {
            affected = true;
            entity.stop_riding();
        }

        if !affected {
            return false;
        }

        self.item_attack_interaction(entity, &damage_source, damage_dealt);
        self.set_last_hurt_mob(Some(target));
        self.cause_food_exhaustion(0.1);
        true
    }

    fn play_sound_holder(&self, holder: Option<&SoundEventHolder>) {
        let Some(sound) = holder.and_then(sound_holder_ref) else {
            return;
        };
        self.play_sound(sound, 1.0, 1.0);
    }

    fn cannot_attack(&self, entity: &dyn Entity) -> bool {
        !entity.attackable() || entity.skip_attack_interaction(self)
    }

    /// Attacks an entity with the player's main-hand base damage.
    ///
    /// Returns `true` if the target accepted damage.
    #[must_use]
    pub fn attack(&self, target: &SharedEntity) -> bool {
        let entity = target.as_ref();
        if self.cannot_attack(entity) {
            return false;
        }

        let attacking_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };
        let (attack_damage, attack_speed, attack_knockback) = {
            let attributes = self.attributes().lock();
            (
                attributes.required_value(vanilla_attributes::ATTACK_DAMAGE) as f32,
                attributes.required_value(vanilla_attributes::ATTACK_SPEED),
                attributes.required_value(vanilla_attributes::ATTACK_KNOCKBACK),
            )
        };
        let attack_strength_delay = Self::attack_strength_delay_from_speed(attack_speed);
        let attack_strength_scale =
            self.attack_strength_scale_for_delay(0.5, attack_strength_delay);
        let damage_source = self.attack_damage_source(&attacking_item);
        let enchantment_context = EnchantmentDamageContext::new(
            entity.entity_type(),
            Some(self.entity_type()),
            Some(self.entity_type()),
            &damage_source,
        );
        let enchanted_damage =
            enchantment_helper::modify_damage(&attacking_item, &enchantment_context, attack_damage);
        let magic_boost = attack_strength_scale * (enchanted_damage - attack_damage);
        let mut base_damage = attack_damage * Self::base_damage_scale_factor(attack_strength_scale);
        base_damage += ITEM_BEHAVIORS
            .get_behavior(attacking_item.item())
            .get_attack_damage_bonus(self, entity, base_damage, &damage_source);
        let total_damage = base_damage + magic_boost;
        let full_strength_attack = attack_strength_scale > 0.9;
        let knockback_attack = self.is_sprinting() && full_strength_attack;
        self.reset_attack_strength_ticker();

        if total_damage <= 0.0 {
            return false;
        }

        // TODO: Apply crits, sweep attacks, damage stats, and sounds.
        let old_movement = entity.velocity();
        let Some(target_world) = entity.level() else {
            return false;
        };
        let was_hurt = entity.hurt(&target_world, &damage_source, total_damage);
        if was_hurt {
            self.set_last_hurt_mob(Some(target));
            let sprint_knockback = if knockback_attack { 0.5 } else { 0.0 };
            self.cause_extra_knockback(
                entity,
                Self::get_knockback(attack_knockback, &attacking_item, &enchantment_context)
                    + sprint_knockback,
                old_movement,
            );
            self.item_attack_interaction(entity, &damage_source, true);
            self.cause_food_exhaustion(0.1);
        }

        let world = self.get_world();
        enchantment_helper::do_post_piercing_attack_effects(&world, self);
        was_hurt
    }

    fn item_attack_interaction(
        &self,
        entity: &dyn Entity,
        damage_source: &DamageSource,
        apply_to_target: bool,
    ) {
        let post_attack_context =
            EnchantmentPostAttackContext::new(entity, Some(self), Some(self), damage_source);
        let (source_item, item_hurt_enemy) = {
            let mut inventory = self.inventory.lock();
            inventory.mutate_item_in_hand(InteractionHand::MainHand, |stack| {
                if stack.is_empty() {
                    return (ItemStack::empty(), false);
                }
                let behavior = ITEM_BEHAVIORS.get_behavior(stack.item());
                if let Some(living_target) = entity.as_living_entity() {
                    behavior.hurt_enemy(stack, living_target, self);
                }
                let source_item = stack.copy_with_count(stack.count());
                (source_item, stack.get_weapon().is_some())
            })
        };

        if apply_to_target {
            let world = self.get_world();
            enchantment_helper::do_post_attack_effects_with_item_source(
                &world,
                entity,
                &source_item,
                &post_attack_context,
            );
        }

        if !item_hurt_enemy {
            return;
        }

        let Some(living_target) = entity.as_living_entity() else {
            return;
        };
        let has_infinite_materials = self.has_infinite_materials();
        let mut inventory = self.inventory.lock();
        inventory.mutate_item_in_hand(InteractionHand::MainHand, |stack| {
            if stack.is_empty() {
                return;
            }
            let behavior = ITEM_BEHAVIORS.get_behavior(stack.item());
            behavior.post_hurt_enemy(stack, living_target, self);
            if let Some(damage) = behavior.item_damage_per_attack(stack) {
                stack.hurt_and_break(damage, has_infinite_materials);
            }
        });
    }

    /// Interacts with an entity using the held item.
    pub fn interact_on(
        &self,
        entity: &dyn Entity,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        if self.is_spectator() {
            // TODO: Open entity menu providers in spectator once that foundation exists.
            return InteractionResult::Pass;
        }

        let inventory_access = InventoryAccess::new(self.inventory.clone(), hand);
        let original_count = inventory_access.with_item(|item| item.count);
        let result = entity.interact(self, hand, location);

        if self.has_infinite_materials() {
            inventory_access.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }

        if result.consumes_action() {
            return result;
        }

        if inventory_access.with_item(|item| item.is_empty()) {
            return InteractionResult::Pass;
        }
        let Some(living_entity) = entity.as_living_entity() else {
            return InteractionResult::Pass;
        };
        let result = living_entity.interact_living_entity_with_equippable(self, hand);
        if self.has_infinite_materials() {
            inventory_access.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }
        if result.consumes_action() {
            return result;
        }

        let item_ref = inventory_access.with_item(|item| item.item());
        let item_behavior = ITEM_BEHAVIORS.get_behavior(item_ref);
        let result = inventory_access.with_item(|item| {
            item_behavior.interact_living_entity(item, self, living_entity, hand)
        });
        if self.has_infinite_materials() {
            inventory_access.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }
        result
    }

    /// Handles a client request to attack an entity.
    pub fn handle_attack(&self, packet: SAttack) {
        if !self.has_client_loaded() || self.is_spectator() {
            return;
        }

        let world = self.get_world();
        let Some(target) = world.get_accessible_entity_by_id(packet.entity_id) else {
            return;
        };

        let target_pos = target.block_position();
        if !world.world_border_snapshot().is_within_bounds_with_margin(
            f64::from(target_pos.x()),
            f64::from(target_pos.z()),
            0.0,
        ) {
            return;
        }

        let main_hand_item = {
            let inventory = self.inventory.lock();
            let stack = inventory.get_item_in_hand(InteractionHand::MainHand);
            stack.copy_with_count(stack.count())
        };

        if !self.is_within_attack_range_with_buffer(
            &main_hand_item,
            target.bounding_box(),
            ATTACK_RANGE_BUFFER,
        ) {
            return;
        }

        if main_hand_item.get_piercing_weapon().is_some() {
            return;
        }

        if Self::is_invalid_attack_target(self.id(), target.id(), target.entity_type()) {
            self.disconnect(Self::invalid_entity_attacked_message());
            log::warn!(
                "Player {} tried to attack an invalid entity",
                self.gameprofile.name
            );
            return;
        }

        if self.cannot_attack_with_item(&main_hand_item, 5) {
            return;
        }

        let _ = self.attack(&target);
    }

    pub(super) fn cannot_attack_with_item(&self, item_stack: &ItemStack, tolerance: i32) -> bool {
        let required_strength = item_stack.minimum_attack_charge();
        if required_strength <= 0.0 {
            return false;
        }

        let optimistic_strength = {
            let ticker = self.tick_state.lock().attack_strength_ticker() + tolerance;
            ticker as f32 / self.current_item_attack_strength_delay()
        };
        optimistic_strength < required_strength
    }

    pub(super) fn is_invalid_attack_target(
        player_id: i32,
        target_id: i32,
        target_type: EntityTypeRef,
    ) -> bool {
        target_id == player_id
            || target_type == &vanilla_entities::ITEM
            || target_type == &vanilla_entities::EXPERIENCE_ORB
    }

    /// Handles a client request to interact with an entity.
    pub fn handle_interact(&self, packet: SInteract) {
        if !self.has_client_loaded() {
            return;
        }

        let world = self.get_world();
        let target = world.get_accessible_entity_by_id(packet.entity_id);
        self.set_crouching(packet.using_secondary_action);
        let Some(target) = target else {
            return;
        };

        let target_pos = target.block_position();
        if !world.world_border_snapshot().is_within_bounds_with_margin(
            f64::from(target_pos.x()),
            f64::from(target_pos.z()),
            0.0,
        ) {
            return;
        }

        if !self.is_within_entity_interaction_range_with_buffer(
            target.bounding_box(),
            ENTITY_INTERACTION_RANGE_BUFFER,
        ) {
            return;
        }

        let result = self.interact_on(target.as_ref(), packet.hand, packet.location);
        if result.should_swing_server() {
            self.swing(packet.hand, true);
        }
        self.broadcast_inventory_changes();
    }
}
