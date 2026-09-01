use std::sync::Arc;

use crate::{entity::LivingEntity as _, world::World};

use super::super::{Abilities, Player, experience::Experience};

impl Player {
    /// Constructs the detached player entity used by vanilla respawn replacement.
    ///
    /// The caller remains responsible for replacing the session binding and world
    /// membership. Chat continuity belongs to [`super::super::PlayerSession`] and
    /// is therefore shared by both incarnations. For a death replacement,
    /// `transfer_inventory` must be `keepInventory || old spectator`.
    pub(crate) fn new_respawn_replacement(
        self: &Arc<Self>,
        target_world: Arc<World>,
        restore_all: bool,
        transfer_inventory: bool,
        spawn_block_valid: bool,
    ) -> Arc<Self> {
        let replacement = Arc::new(Self::new(
            self.gameprofile.clone(),
            Arc::clone(&self.connection),
            Arc::clone(&self.session),
            target_world,
            self.server.clone(),
            Arc::clone(&self.config),
            self.base.id(),
            self.client_information(),
        ));

        replacement.restore_respawn_state_from(
            self,
            restore_all,
            transfer_inventory,
            spawn_block_valid,
        );
        replacement.copy_respawn_scoreboard_tags_from(self);
        replacement
    }

    fn restore_respawn_state_from(
        &self,
        old_player: &Self,
        restore_all: bool,
        transfer_inventory: bool,
        spawn_block_valid: bool,
    ) {
        let game_mode = old_player.game_mode();
        self.restore_game_modes(game_mode, old_player.previous_game_mode());
        {
            let mut abilities = self.abilities.lock();
            *abilities = Abilities::default();
            abilities.update_for_game_mode(game_mode);
        }

        {
            let old_attributes = old_player.living_base.attributes().lock();
            let mut attributes = self.living_base.attributes().lock();
            attributes.assign_base_values(&old_attributes);
            if restore_all {
                attributes.assign_permanent_modifiers(&old_attributes);
            }
        }

        let permission_state = old_player.permissions.lock().clone();
        *self.permissions.lock() = permission_state;
        {
            let old_stats = old_player.stats.lock();
            self.stats.lock().stats.clone_from(&old_stats.stats);
        }
        *self.seen_credits.lock() = *old_player.seen_credits.lock();
        let residence = old_player.residence.lock().clone();
        *self.residence.lock() = residence;

        // Vanilla keeps the packet listener across respawn, so its teleport ID
        // remains monotonic even though the pending position belongs to the new player.
        self.teleport_state.lock().teleport_id = old_player.teleport_state.lock().teleport_id;

        if spawn_block_valid {
            let old_respawn_config = old_player.respawn_config.lock();
            self.respawn_config.lock().clone_from(&old_respawn_config);
        }

        if restore_all {
            self.restore_all_respawn_state_from(old_player);
        } else {
            self.set_health(self.get_max_health());
            if transfer_inventory {
                self.restore_inventory_experience_and_score_from(old_player);
            }
        }
    }

    fn restore_all_respawn_state_from(&self, old_player: &Self) {
        self.set_health(old_player.get_health());
        *self.food_data.lock() = old_player.food_data.lock().clone();

        for effect in old_player.living_base.active_mob_effects() {
            self.add_mob_effect(effect);
        }

        self.restore_inventory_experience_and_score_from(old_player);
        self.base
            .set_portal_process(old_player.base.portal_process());
    }

    fn restore_inventory_experience_and_score_from(&self, old_player: &Self) {
        let inventory = old_player.inventory.lock().replacement_copy();
        *self.inventory.lock() = inventory;
        let experience = old_player.experience.lock();
        *self.experience.lock() = Experience::from_parts(
            experience.level(),
            experience.progress(),
            experience.total_points(),
        );
        self.set_score(old_player.score());
    }

    fn copy_respawn_scoreboard_tags_from(&self, old_player: &Self) {
        for tag in old_player.base.tags() {
            self.base.add_tag(tag);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{
        init_vanilla_registry, item_stack::ItemStack, vanilla_attributes, vanilla_custom_stats,
        vanilla_items, vanilla_mob_effects,
    };
    use steel_utils::{BlockPos, Identifier, types::GameType};

    use crate::{
        entity::{
            Entity as _, LivingEntity as _, MobEffectInstance,
            attribute::{AttributeModifier, AttributeModifierOperation},
        },
        inventory::container::Container as _,
        level_data::RespawnData,
        player::{ClientInformation, PlayerRespawnConfig, experience::Experience},
        portal::{PortalKind, PortalProcessor},
        test_support::{TestPlayerBuilder, fresh_test_world},
    };

    const ENTITY_ID: i32 = 17;

    #[test]
    fn replacement_is_fresh_detached_entity_in_same_session() {
        init_vanilla_registry();
        let source_world = fresh_test_world("respawn_restore_identity_source");
        let target_world = fresh_test_world("respawn_restore_identity_target");
        let client_information = ClientInformation {
            language: "en_gb".to_owned(),
            view_distance: 3,
            ..ClientInformation::default()
        };
        let old_player = TestPlayerBuilder::new(source_world, "Respawning", ENTITY_ID)
            .client_information(client_information)
            .build();
        old_player.teleport_state.lock().teleport_id = 41;

        let replacement =
            old_player.new_respawn_replacement(Arc::clone(&target_world), false, false, true);

        assert_eq!(replacement.id(), old_player.id());
        assert_eq!(replacement.uuid(), old_player.uuid());
        assert_ne!(replacement.instance_id(), old_player.instance_id());
        assert!(!Arc::ptr_eq(&replacement, &old_player));
        assert!(Arc::ptr_eq(&replacement.connection, &old_player.connection));
        assert!(Arc::ptr_eq(&replacement.session, &old_player.session));
        assert!(Arc::ptr_eq(&replacement.config, &old_player.config));
        assert!(Arc::ptr_eq(&replacement.get_world(), &target_world));
        assert_eq!(replacement.client_information().language, "en_gb");
        assert_eq!(replacement.client_information().view_distance, 3);
        assert_eq!(replacement.teleport_state.lock().teleport_id, 41);
        replacement.synchronize_respawn_replacement(DVec3::new(1.0, 64.0, 1.0), (0.0, 0.0));
        let mut teleport_state = replacement.teleport_state.lock();
        assert_eq!(teleport_state.teleport_id, 42);
        assert!(teleport_state.try_accept(41).is_none());
        assert!(teleport_state.try_accept(42).is_some());
        drop(teleport_state);
        assert!(old_player.session.is_current_player(&old_player));
        assert!(!old_player.session.is_current_player(&replacement));
        assert!(!target_world.contains_player(&replacement));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test keeps the vanilla restore-all state table in one comparison"
    )]
    fn restore_all_copies_vanilla_restore_state_without_mutating_source() {
        init_vanilla_registry();
        let source_world = fresh_test_world("respawn_restore_all_source");
        let target_world = fresh_test_world("respawn_restore_all_target");
        let old_player = TestPlayerBuilder::new(source_world, "Credits", ENTITY_ID).build();
        let permanent_modifier_id = Identifier::vanilla_static("respawn_restore_test");

        old_player.restore_game_modes(GameType::Spectator, Some(GameType::Creative));
        old_player.abilities.lock().walking_speed = 0.7;
        {
            let mut attributes = old_player.attributes().lock();
            attributes.set_base_value(vanilla_attributes::MAX_HEALTH, 30.0);
            assert!(attributes.add_modifier(
                vanilla_attributes::MAX_HEALTH,
                AttributeModifier {
                    id: permanent_modifier_id.clone(),
                    amount: 5.0,
                    operation: AttributeModifierOperation::AddValue,
                },
                true,
            ));
        }
        old_player.set_health(12.0);
        old_player.food_data.lock().food_level = 7;
        assert!(old_player.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::SPEED,
            200,
            1,
        )));
        old_player
            .inventory
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::OAK_LOG, 4));
        *old_player.experience.lock() = Experience::from_parts(8, 0.25, 91);
        old_player.experience.lock().dirty = false;
        old_player.set_score(42);
        old_player
            .base
            .set_portal_process(Some(PortalProcessor::new(
                PortalKind::Nether,
                BlockPos::new(1, 64, 1),
            )));
        old_player.award_custom_stat_with_count(&vanilla_custom_stats::JUMP, 9);
        old_player
            .permissions
            .lock()
            .groups
            .push("builders".to_owned());
        old_player.permissions.lock().version = 7;
        *old_player.seen_credits.lock() = true;
        *old_player.won_game.lock() = true;
        old_player.base.add_tag("credits-return".to_owned());
        let respawn_config = PlayerRespawnConfig::new(
            RespawnData::of(target_world.key.clone(), BlockPos::new(4, 70, 6), 90.0, 0.0),
            false,
        );
        *old_player.respawn_config.lock() = Some(respawn_config.clone());
        old_player
            .base
            .set_position_local(DVec3::new(10.0, 80.0, 10.0));
        old_player.mark_joined_world();

        let replacement = old_player.new_respawn_replacement(target_world, true, false, true);

        assert_eq!(replacement.game_mode(), GameType::Spectator);
        assert_eq!(replacement.previous_game_mode(), Some(GameType::Creative));
        let abilities = replacement.abilities.lock();
        assert!(abilities.invulnerable);
        assert!(abilities.may_fly);
        assert!(abilities.flying);
        assert_eq!(abilities.walking_speed, 0.1);
        drop(abilities);
        assert_eq!(replacement.get_health().to_bits(), 12.0_f32.to_bits());
        assert_eq!(replacement.get_max_health().to_bits(), 35.0_f32.to_bits());
        assert!(
            replacement
                .attributes()
                .lock()
                .has_modifier(vanilla_attributes::MAX_HEALTH, &permanent_modifier_id)
        );
        assert_eq!(replacement.food_data.lock().food_level, 7);
        assert_eq!(
            replacement.mob_effect(vanilla_mob_effects::SPEED),
            old_player.mob_effect(vanilla_mob_effects::SPEED)
        );
        assert_eq!(replacement.inventory.lock().get_item(0).count(), 4);
        assert_eq!(replacement.experience.lock().level(), 8);
        assert_eq!(
            replacement.experience.lock().progress().to_bits(),
            0.25_f32.to_bits()
        );
        assert_eq!(replacement.experience.lock().total_points(), 91);
        assert!(replacement.experience.lock().dirty);
        assert!(!old_player.experience.lock().dirty);
        assert_eq!(replacement.score(), 42);
        assert_eq!(
            replacement.base.portal_process(),
            old_player.base.portal_process()
        );
        assert_eq!(replacement.stats(), old_player.stats());
        assert_eq!(replacement.permissions.lock().groups, ["builders"]);
        assert_eq!(replacement.permissions.lock().version, 7);
        assert!(*replacement.seen_credits.lock());
        assert_eq!(replacement.respawn_config(), Some(respawn_config));
        assert_eq!(replacement.tags(), ["credits-return"]);
        assert_eq!(replacement.position(), DVec3::ZERO);
        assert!(!replacement.has_joined_world());
        assert!(!*replacement.won_game.lock());

        replacement.inventory.lock().clear_content();
        replacement.food_data.lock().food_level = 2;
        assert!(replacement.remove_mob_effect(vanilla_mob_effects::SPEED));
        assert_eq!(old_player.inventory.lock().get_item(0).count(), 4);
        assert_eq!(old_player.food_data.lock().food_level, 7);
        assert!(old_player.has_mob_effect(vanilla_mob_effects::SPEED));
    }

    #[test]
    fn death_restore_keeps_fresh_state_unless_inventory_transfer_is_requested() {
        init_vanilla_registry();
        let source_world = fresh_test_world("respawn_restore_death_source");
        let target_world = fresh_test_world("respawn_restore_death_target");
        let old_player = TestPlayerBuilder::new(source_world, "Death", ENTITY_ID).build();
        let permanent_modifier_id = Identifier::vanilla_static("death_restore_test");

        {
            let mut attributes = old_player.attributes().lock();
            attributes.set_base_value(vanilla_attributes::MAX_HEALTH, 30.0);
            assert!(attributes.add_modifier(
                vanilla_attributes::MAX_HEALTH,
                AttributeModifier {
                    id: permanent_modifier_id.clone(),
                    amount: 5.0,
                    operation: AttributeModifierOperation::AddValue,
                },
                true,
            ));
        }
        old_player.set_health(0.0);
        old_player.food_data.lock().food_level = 4;
        assert!(old_player.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::SPEED,
            100,
            0,
        )));
        old_player
            .inventory
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 3));
        *old_player.experience.lock() = Experience::from_parts(4, 0.5, 36);
        old_player.experience.lock().dirty = false;
        old_player.set_score(11);
        old_player
            .base
            .set_portal_process(Some(PortalProcessor::new(
                PortalKind::Nether,
                BlockPos::new(2, 64, 2),
            )));
        *old_player.respawn_config.lock() = Some(PlayerRespawnConfig::new(
            RespawnData::of(target_world.key.clone(), BlockPos::new(8, 70, 8), 0.0, 0.0),
            false,
        ));

        let fresh =
            old_player.new_respawn_replacement(Arc::clone(&target_world), false, false, false);
        let transferred = old_player.new_respawn_replacement(target_world, false, true, true);

        assert_eq!(fresh.get_health().to_bits(), 30.0_f32.to_bits());
        assert!(
            !fresh
                .attributes()
                .lock()
                .has_modifier(vanilla_attributes::MAX_HEALTH, &permanent_modifier_id)
        );
        assert_eq!(fresh.food_data.lock().food_level, 20);
        assert!(!fresh.has_mob_effect(vanilla_mob_effects::SPEED));
        assert!(fresh.inventory.lock().get_item(0).is_empty());
        assert_eq!(fresh.experience.lock().total_points(), 0);
        assert_eq!(fresh.score(), 0);
        assert_eq!(fresh.base.portal_process(), None);
        assert_eq!(fresh.respawn_config(), None);

        assert_eq!(transferred.get_health().to_bits(), 30.0_f32.to_bits());
        assert_eq!(transferred.inventory.lock().get_item(0).count(), 3);
        assert_eq!(transferred.experience.lock().level(), 4);
        assert_eq!(transferred.experience.lock().total_points(), 36);
        assert!(transferred.experience.lock().dirty);
        assert_eq!(transferred.score(), 11);
        assert_eq!(transferred.respawn_config(), old_player.respawn_config());

        assert_eq!(old_player.get_health().to_bits(), 0.0_f32.to_bits());
        assert_eq!(old_player.inventory.lock().get_item(0).count(), 3);
        assert_eq!(old_player.experience.lock().total_points(), 36);
        assert!(old_player.has_mob_effect(vanilla_mob_effects::SPEED));
    }
}
