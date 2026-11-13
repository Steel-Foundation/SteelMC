//! `ServerPlayer`'s overrides of the living hooks.

use super::*;
use crate::entity::LivingTable;
use crate::entity::prelude::*;

impl Player {
    fn living_indicate_damage(&self, xd: f64, zd: f64) {
        let (y_rot, _) = self.rotation();
        let hurt_dir = zd.atan2(xd) * RAD_TO_DEG_F64 - f64::from(y_rot);
        self.send_packet(CHurtAnimation {
            entity_id: self.id(),
            yaw: hurt_dir as f32,
        });
    }

    pub(super) fn tick_player(&self) {
        self.advance_tick();
        self.tick_item_cooldowns();
        self.tick_attack_strength();
        self.tick_throttlers();
        self.check_idle_timeout();
        self.tick_client_load_timeout();
        self.tick_sleep_counter();
        if self.living().is_sleeping() {
            let world = self.get_world();
            let state = self
                .living()
                .sleeping_pos()
                .map(|pos| world.get_block_state(pos));
            if state.is_some_and(|state| {
                !self.bed_rule_value_allows(Self::bed_rule_for(&world, state).can_sleep)
            }) {
                self.stop_sleep_in_bed(false, true);
            } else if !self.can_interact_with_level()
                || self
                    .living()
                    .sleeping_pos()
                    .is_none_or(|pos| !world.get_block_state(pos).is_bed())
            {
                self.stop_sleep_in_bed(true, true);
            }
        }

        self.set_no_physics(self.is_spectator());
        if self.is_spectator() || self.is_passenger() {
            self.set_on_ground(false);
        }

        let tick_position = self.position();

        self.movement.lock().reset_for_tick(tick_position);
        self.set_old_position_to_current();
        self.reset_vehicle_movement_for_tick();

        self.base_tick();
        self.living().detect_equipment_updates();
        if !self.is_removed() {
            let _movement = self.living().ai_step();
        }

        // Vanilla snaps the player back to firstGood after ServerPlayer.doTick().
        if let Err(error) = self.try_set_position(tick_position) {
            panic!(
                "failed to restore player {} tick position after ai_step: {error}",
                self.id()
            );
        }
        self.refresh_fluid_contact();

        self.tick_ack_block_changes();

        self.living_base.decrement_invulnerable_time();
        self.living().tick_mob_effects();
        self.tick_active_item_use();
        self.tick_stats();

        if self.living().health() <= 0.0 {
            self.tick_death();
        } else {
            let world = self.get_world();
            self.block_breaking.lock().tick(self, &world);

            self.update_player_attributes();
            self.advancements.lock().flush_dirty(self, true);
            self.living_base.refresh_speed_from_attributes();
            self.tick_food_data();

            if self.living().is_sprinting() && !self.food_data.lock().has_enough_food() {
                self.living().set_sprinting(false);
            }
        }

        if self.disconnect_if_floating_too_long() {
            return;
        }
        if self.disconnect_if_vehicle_floating_too_long() {
            return;
        }

        self.living().tick_living_state();

        self.tick_open_menu();
        self.flush_inventory_resync();
        self.broadcast_inventory_changes();
        self.update_pose();

        {
            let health = self.living().health();
            let (food, saturation) = {
                let food_data = self.food_data.lock();
                (food_data.food_level, food_data.saturation_level)
            };

            let saturation_zero = saturation == 0.0;

            let mut sync = self.health_sync.lock();
            if sync.needs_update(health, food, saturation_zero) {
                self.send_packet(CSetHealth {
                    health,
                    food,
                    food_saturation: saturation,
                });
                sync.record_sent(health, food, saturation_zero);
            }
        }

        self.send_experience_packet_if_dirty();

        self.connection.tick();
    }

    fn living_is_using_item(&self) -> bool {
        self.living_base.is_using_item()
    }

    fn living_get_luck(&self) -> f32 {
        self.living()
            .attributes()
            .lock()
            .required_value(vanilla_attributes::LUCK) as f32
    }

    /// `Player.isInvulnerableTo`: after `super`, damage types switched off by
    /// game rules, then `ServerPlayer`'s players whose client has not loaded.
    // TODO: `ServerPlayer` also protects a player changing dimension from
    // everything but ender pearl damage; the player does not track that state yet.
    fn living_is_invulnerable_to(&self, world: &World, source: &DamageSource) -> bool {
        if living_entity::is_invulnerable_to(self, world, source) {
            return true;
        }
        let rule_disabled =
            Self::disabled_damage_game_rule(source).is_some_and(|rule| !world.get_game_rule(rule));
        rule_disabled || !self.has_client_loaded()
    }

    fn living_hurt_armor(&self, source: &DamageSource, damage: f32) {
        self.living().do_hurt_equipment(
            source,
            damage,
            &[
                EquipmentSlot::Feet,
                EquipmentSlot::Legs,
                EquipmentSlot::Chest,
                EquipmentSlot::Head,
            ],
        );
    }

    fn living_actually_hurt(&self, world: &World, source: &DamageSource, amount: f32) {
        Player::actually_hurt(self, world, source, amount);
    }

    fn living_hurt_broadcast_chunk(&self) -> ChunkPos {
        *self.last_chunk_pos.lock()
    }

    fn living_die(&self, source: &DamageSource) {
        Player::die(self, source);
    }

    fn living_tick_equipment(&self) {
        // skip main hand because its already being ticked through player inventory
        InventoryTickContext::tick_equipment(
            &self.get_world(),
            &self.living(),
            EquipmentSlot::ALL
                .into_iter()
                .filter(|slot| *slot != EquipmentSlot::MainHand),
        );
    }

    fn living_with_equipment_slot(&self, slot: EquipmentSlot, visitor: &mut dyn FnMut(&ItemStack)) {
        let inventory = self.inventory.lock();
        visitor(inventory.get_ref(slot));
    }

    fn living_with_equipment_slot_mut(
        &self,
        slot: EquipmentSlot,
        visitor: &mut dyn FnMut(&mut ItemStack),
    ) {
        let mut inventory = self.inventory.lock();
        inventory.with_equipment_item_mut(slot, visitor);
    }

    fn living_interact_living_entity_with_equippable(
        &self,
        player: &Player,
        hand: InteractionHand,
    ) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.clone()
        };
        let Some(equippable) = item_stack.get_equippable() else {
            return InteractionResult::Pass;
        };
        if !equippable.equip_on_interact {
            return InteractionResult::Pass;
        }

        let slot = equippable.slot;
        let can_equip = |stack: &ItemStack| {
            stack.get_equippable().is_some_and(|equippable| {
                equippable.equip_on_interact
                    && equippable.slot == slot
                    && self.living().is_equippable_in_slot(stack, slot)
            })
        };
        if !can_equip(&item_stack) || !EntityHooks::is_alive(self) {
            return InteractionResult::Pass;
        }

        let source_ref = ContainerRef::from(Arc::clone(&player.inventory));
        let target_ref = ContainerRef::from(Arc::clone(&self.inventory));
        let source_id = source_ref.container_id();
        let target_id = target_ref.container_id();
        let mut guard = ContainerLockGuard::lock_all(&[source_ref, target_ref]);
        let source_slot = match hand {
            InteractionHand::MainHand => EquipmentSlot::MainHand,
            InteractionHand::OffHand => EquipmentSlot::OffHand,
        };

        let equipped = if source_id == target_id {
            let Some(inventory) = guard.get_typed_mut::<PlayerInventory>(source_id) else {
                unreachable!("player inventory container retains its concrete type");
            };
            if !can_equip(inventory.get_item_in_hand(hand)) || !inventory.get_ref(slot).is_empty() {
                return InteractionResult::Pass;
            }

            let equipped = inventory.get_mut(source_slot).split(1);
            if equipped.is_empty() {
                return InteractionResult::Pass;
            }
            let equipped_for_effects = equipped.copy_with_count(1);
            *inventory.get_mut(slot) = equipped;
            equipped_for_effects
        } else {
            let Some((source_inventory, target_inventory)) =
                guard.get_two_typed_mut::<PlayerInventory, PlayerInventory>(source_id, target_id)
            else {
                unreachable!("player inventory containers retain their concrete type");
            };
            if !can_equip(source_inventory.get_item_in_hand(hand))
                || !target_inventory.get_ref(slot).is_empty()
            {
                return InteractionResult::Pass;
            }

            let equipped = source_inventory.get_mut(source_slot).split(1);
            if equipped.is_empty() {
                return InteractionResult::Pass;
            }
            let equipped_for_effects = equipped.copy_with_count(1);
            *target_inventory.get_mut(slot) = equipped;
            equipped_for_effects
        };
        drop(guard);

        player.inventory.lock().set_changed();
        if source_id != target_id {
            self.inventory.lock().set_changed();
        }

        self.living()
            .on_equip_item(slot, &ItemStack::empty(), &equipped);
        // TODO: Emit EQUIP game event once game-event dispatch is implemented.
        InteractionResult::Success
    }

    fn living_has_infinite_materials(&self) -> bool {
        Player::has_infinite_materials(self)
    }

    fn living_handle_extra_items_created_on_use(&self, extra: ItemStack) {
        let leftover = self.inventory.lock().add_or_return(extra);
        if !leftover.is_empty() {
            let _ = self.drop_item(leftover, false, Prediction::Predicted);
        }
    }

    fn living_get_absorption_amount(&self) -> f32 {
        *self.entity_data.lock().player_absorption.get()
    }

    fn living_set_absorption_amount(&self, amount: f32) {
        let max_absorption = self
            .living_base
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MAX_ABSORPTION) as f32;
        self.entity_data
            .lock()
            .player_absorption
            .set(amount.clamp(0.0, max_absorption));
    }

    fn living_is_affected_by_fluids(&self) -> bool {
        !self.is_flying()
    }

    fn living_stop_sleeping(&self) {
        self.stop_sleep_in_bed(true, true);
    }

    /// `Player.jumpFromGround`: after `super`, the jump statistic and exhaustion.
    fn living_jump_from_ground(&self) {
        living_entity::jump_from_ground(self);
        self.award_custom_stat(&vanilla_custom_stats::JUMP);
        if self.living().is_sprinting() {
            self.cause_food_exhaustion(food_constants::EXHAUSTION_SPRINT_JUMP);
        } else {
            self.cause_food_exhaustion(food_constants::EXHAUSTION_JUMP);
        }
    }

    /// `Player.aiStep` around `super.aiStep()`.
    fn living_ai_step(&self) -> Option<MoveResult> {
        self.tick_regeneration();
        InventoryTickContext::tick_player_inventory(&self.get_world(), self);
        if self.is_flying() && !self.is_passenger() {
            self.reset_fall_distance();
        }

        let result = living_entity::ai_step(self);
        self.living().set_y_head_rot(self.rotation().0);
        // Pickup uses the simulated position; `tick_living_entity` restores the
        // authoritative tick position after this returns.
        if self.living().health() > 0.0 {
            self.touch_nearby_items();
        }
        result
    }

    /// `Player.travel` around `super.travel()`: swimming follows the look
    /// direction, and flying keeps most vertical speed.
    fn living_travel(&self, input: DVec3) -> Option<MoveResult> {
        if self.is_passenger() {
            return living_entity::travel(self, input);
        }

        if self.is_swimming() {
            let look_angle_y = self.look_angle().y;
            let multiplier = if look_angle_y < -0.2 { 0.085 } else { 0.06 };
            let has_fluid_above = self.level().is_some_and(|world| {
                let position = self.position();
                let pos = BlockPos::containing(position.x, position.y + 0.9, position.z);
                !get_fluid_state(&world, pos).is_empty()
            });
            if look_angle_y <= 0.0 || self.living().is_jumping() || has_fluid_above {
                let velocity = self.velocity();
                self.set_velocity(
                    velocity + DVec3::new(0.0, (look_angle_y - velocity.y) * multiplier, 0.0),
                );
            }
        }

        if self.is_flying() {
            let original_movement_y = self.velocity().y;
            let result = living_entity::travel(self, input);
            let velocity = self.velocity();
            self.set_velocity(DVec3::new(
                velocity.x,
                original_movement_y * 0.6,
                velocity.z,
            ));
            result
        } else {
            living_entity::travel(self, input)
        }
    }

    fn living_get_flying_speed(&self) -> f32 {
        if self.is_flying() && !self.is_passenger() {
            let flying_speed = self.abilities.lock().flying_speed;
            if self.living().is_sprinting() {
                flying_speed * 2.0
            } else {
                flying_speed
            }
        } else if self.living().is_sprinting() {
            0.025_999_999
        } else {
            0.02
        }
    }

    fn living_on_equipped_item_broken(&self, item: ItemRef, slot: EquipmentSlot) {
        self.broadcast_entity_event(slot.into());
        self.living().refresh_equipment_attribute_modifiers(slot);
        self.award_stat(&vanilla_stat_types::ITEM_BROKEN, item);
    }
}

impl LivingTable for Player {
    const LIVING: LivingProps<Self> = LivingProps {
        // `Player.getVoicePitch` never calls `super`.
        voice_pitch: |_| 1.0,
        // `Player.canBeSeenAsEnemy`: invulnerable players are never enemies.
        can_be_seen_as_enemy: |player| {
            !player.abilities.lock().invulnerable && living_entity::can_be_seen_as_enemy(player)
        },
        is_invulnerable_to: Self::living_is_invulnerable_to,
        // `Player.canGlide`: flying players do not glide.
        can_glide: |player| !player.is_flying() && living_entity::can_glide(player),
        // `Player.isImmobile`: sleeping players are immobile too.
        is_immobile: |player| living_entity::is_immobile(player) || player.living().is_sleeping(),
        jump_from_ground: Self::living_jump_from_ground,
        ai_step: Self::living_ai_step,
        travel: Self::living_travel,
        default_dimensions: |_, pose| Player::dimensions_for_pose(pose),
        hurt_sound: |_, source| Some(source.damage_type.effects.sound()),
        does_emit_equip_event: |_, slot| slot.slot_type() == EquipmentSlotType::HumanoidArmor,
        hurt_helmet: |player, source, damage| {
            player
                .living()
                .do_hurt_equipment(source, damage, &[EquipmentSlot::Head]);
        },
        death_sound: |_| Some(&sound_events::ENTITY_PLAYER_DEATH),
        dismount_poses: |_| {
            &[
                EntityPose::Standing,
                EntityPose::Sneaking,
                EntityPose::Swimming,
            ]
        },
        indicate_damage: Self::living_indicate_damage,
        is_using_item: Self::living_is_using_item,
        luck: Self::living_get_luck,
        hurt_armor: Self::living_hurt_armor,
        actually_hurt: Self::living_actually_hurt,
        hurt_broadcast_chunk: Self::living_hurt_broadcast_chunk,
        die: Self::living_die,
        tick_equipment: Self::living_tick_equipment,
        with_equipment_slot: Self::living_with_equipment_slot,
        with_equipment_slot_mut: Self::living_with_equipment_slot_mut,
        interact_living_entity_with_equippable: Self::living_interact_living_entity_with_equippable,
        has_infinite_materials: Self::living_has_infinite_materials,
        handle_extra_items_created_on_use: Self::living_handle_extra_items_created_on_use,
        absorption_amount: Self::living_get_absorption_amount,
        set_absorption_amount: Self::living_set_absorption_amount,
        is_affected_by_fluids: Self::living_is_affected_by_fluids,
        stop_sleeping: Self::living_stop_sleeping,
        flying_speed: Self::living_get_flying_speed,
        on_equipped_item_broken: Self::living_on_equipped_item_broken,
        ..LivingProps::new()
    };
}
