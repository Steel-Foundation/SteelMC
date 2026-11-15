use super::{
    Abilities, Arc, ArcSwap, AtomicBool, BlockBreakingManager, BlockPos, CLevelEvent, CheckedMutex,
    FoodData, HealthSyncState, ItemCooldowns, ItemStack, MovementState, Ordering, Player,
    PlayerEntityData, PlayerInventorySyncState, PlayerResidenceState, PortalTicketTarget,
    RemovalReason, ResetReason, SharedEntity, TeleportPostAction, TeleportPostTransition,
    TeleportState, TeleportTransition, Weak, WeakSelf, World, inventory_menu, level_events,
    player_inventory, vanilla_entities,
};
use crate::entity::prelude::*;

impl Player {
    /// Builds this player's replacement in `world` and moves every piece of
    /// state into it, leaving this incarnation an empty shell.
    ///
    /// Vanilla keeps the same `ServerPlayer` across a dimension change. Steel
    /// rebuilds the entity so that an entity's world never changes, and moves
    /// the state rather than copying it so the replacement is the same player
    /// in every other respect. The caller detaches this player from its world
    /// first, and rebinds the session and the player registries afterwards.
    pub(crate) fn moved_to_world(self: &Arc<Self>, world: Arc<World>) -> Arc<Self> {
        let world_ref = Arc::downgrade(&world);
        let replacement = WeakSelf::build(|this| Self {
            gameprofile: self.gameprofile.clone(),
            connection: Arc::clone(&self.connection),
            session: Arc::clone(&self.session),
            world: ArcSwap::new(world),
            server: Weak::clone(&self.server),
            config: Arc::clone(&self.config),
            base: self
                .base
                .moved_to_world(world_ref, this)
                .with_swappable_world(),
            lifecycle: CheckedMutex::new(*self.lifecycle.lock()),
            movement: CheckedMutex::new(self.movement.replace(MovementState::new())),
            entity_data: CheckedMutex::new(self.entity_data.replace(PlayerEntityData::new())),
            last_chunk_pos: CheckedMutex::new(*self.last_chunk_pos.lock()),
            last_tracking_view: CheckedMutex::new(*self.last_tracking_view.lock()),
            game_modes: CheckedMutex::new(*self.game_modes.lock()),
            inventory: Arc::clone(&self.inventory),
            inventory_sync: CheckedMutex::new(
                self.inventory_sync.replace(PlayerInventorySyncState::new()),
            ),
            ender_chest_inventory: Arc::clone(&self.ender_chest_inventory),
            last_item_in_main_hand: CheckedMutex::new(
                self.last_item_in_main_hand.replace(ItemStack::empty()),
            ),
            inventory_menu: CheckedMutex::new(
                self.inventory_menu
                    .replace(inventory_menu(Arc::clone(&self.inventory))),
            ),
            open_menu: CheckedMutex::new(
                self.open_menu
                    .replace(player_inventory::OpenMenuState::new()),
            ),
            container_counter: CheckedMutex::new(*self.container_counter.lock()),
            teleport_state: CheckedMutex::new(self.teleport_state.replace(TeleportState::new())),
            item_cooldowns: CheckedMutex::new(
                self.item_cooldowns.replace(ItemCooldowns::default()),
            ),
            tick_state: CheckedMutex::new(*self.tick_state.lock()),
            sleep_state: CheckedMutex::new(*self.sleep_state.lock()),
            respawn_config: CheckedMutex::new(self.respawn_config.replace(None)),
            abilities: CheckedMutex::new(self.abilities.replace(Abilities::default())),
            block_breaking: CheckedMutex::new(
                self.block_breaking.replace(BlockBreakingManager::new()),
            ),
            living_base: self.living_base.moved(&vanilla_entities::PLAYER),
            food_data: CheckedMutex::new(self.food_data.replace(FoodData::new())),
            health_sync: CheckedMutex::new(self.health_sync.replace(HealthSyncState::new())),
            experience: CheckedMutex::new(*self.experience.lock()),
            seen_credits: AtomicBool::new(self.seen_credits.load(Ordering::Relaxed)),
            spawn_extra_particles_on_fall: AtomicBool::new(
                self.spawn_extra_particles_on_fall.load(Ordering::Relaxed),
            ),
            won_game: AtomicBool::new(self.won_game.load(Ordering::Relaxed)),
            chunk_send_epoch: CheckedMutex::new(*self.chunk_send_epoch.lock()),
            residence: CheckedMutex::new(self.residence.replace(PlayerResidenceState::new())),
            ender_pearls: CheckedMutex::new(self.ender_pearls.replace(Vec::new())),
            fishing: CheckedMutex::new(self.fishing.replace(None)),
            last_action_time: CheckedMutex::new(*self.last_action_time.lock()),
        });

        let replacement_entity: SharedEntity = Arc::<Self>::clone(&replacement);
        for pearl in replacement.ender_pearls() {
            if pearl.projectile_owner_uuid() == Some(replacement.gameprofile.id()) {
                pearl.restore_owner_reference(&replacement_entity);
            }
        }
        self.base.set_removed(RemovalReason::ChangedWorld);
        replacement
    }

    fn apply_post_teleport_transition(&self, post_transition: &TeleportPostTransition) {
        for action in post_transition.actions() {
            match *action {
                TeleportPostAction::PlayPortalSound => {
                    self.send_packet(CLevelEvent::new(
                        level_events::SOUND_PORTAL_TRAVEL,
                        BlockPos::ZERO,
                        0,
                        false,
                    ));
                }
                TeleportPostAction::PlacePortalTicket(target) => {
                    let ticket_position = match target {
                        PortalTicketTarget::Destination => BlockPos::from(self.position()),
                        PortalTicketTarget::Block(pos) => pos,
                    };
                    self.get_world().place_portal_ticket(ticket_position);
                }
            }
        }
    }

    /// Applies an ordinary player transition that has already passed server world-change checks.
    /// Cross-domain player state is restored only by the domain-switch workflow.
    ///
    /// Returns the player to carry on with: this one after a same-world
    /// transition, its replacement after a change of world.
    pub(crate) fn change_world_within_domain(
        self: &Arc<Self>,
        teleport_transition: &TeleportTransition,
    ) -> Option<Arc<Self>> {
        let current_world = self.get_world();
        let new_world = &teleport_transition.target_world;
        if current_world.domain() != new_world.domain() {
            tracing::error!(
                entity_id = self.id(),
                source_domain = current_world.domain(),
                target_domain = new_world.domain(),
                "Refusing player world change outside the domain-switch workflow"
            );
            return None;
        }

        let current_position = self.position();
        let current_rotation = self.rotation();
        let current_velocity = self.velocity();
        let position = teleport_transition.resolved_position(current_position);
        let rotation = teleport_transition.resolved_rotation(current_rotation);
        let velocity =
            teleport_transition.resolved_velocity(current_velocity, current_rotation, rotation);
        self.set_portal_cooldown(teleport_transition.portal_cooldown);
        if !teleport_transition.as_passenger {
            self.stop_riding();
        }
        let player = if Arc::ptr_eq(&current_world, new_world) {
            if let Err(error) = self.teleport_with_velocity_packet(
                position,
                velocity,
                rotation,
                teleport_transition.position,
                teleport_transition.velocity,
                teleport_transition.rotation,
                teleport_transition.relatives,
            ) {
                panic!(
                    "failed to commit same-world portal teleport for player {}: {error}",
                    self.id()
                );
            }
            self.reset_flying_ticks();
            Arc::clone(self)
        } else {
            let player = self.rebuild_in_world(new_world)?;
            if !player.spawn_with_velocity_packet(
                position,
                rotation,
                velocity,
                ResetReason::WorldChange,
                teleport_transition.position,
                teleport_transition.rotation,
                teleport_transition.velocity,
                teleport_transition.relatives,
            ) {
                return None;
            }
            // Vanilla: PlayerList.sendAllPlayerInfo -> inventoryMenu.sendAllDataToRemote
            player.send_inventory_to_remote();
            player
        };
        player.apply_post_teleport_transition(&teleport_transition.post_transition);
        Some(player)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::prelude::*;
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{
        init_vanilla_registry, item_stack::ItemStack, vanilla_items, vanilla_mob_effects,
    };

    use crate::{
        behavior::init_behaviors,
        entity::MobEffectInstance,
        inventory::container::Container as _,
        player::experience::Experience,
        test_support::{TestPlayerBuilder, fresh_test_world},
    };

    #[test]
    fn moved_player_is_the_same_player_in_the_new_world() {
        init_vanilla_registry();
        init_behaviors();
        let source_fixture = fresh_test_world("moved_player_source");
        let target_fixture = fresh_test_world("moved_player_target");
        let target_world = &target_fixture.world;
        let old_player =
            TestPlayerBuilder::new(Arc::clone(&source_fixture.world), "Moving", 23).build();
        old_player
            .base
            .set_position_local(DVec3::new(10.0, 80.0, 10.0));
        old_player.living().set_health(7.0);
        old_player.food_data.lock().food_level = 5;
        old_player
            .living()
            .add_mob_effect(MobEffectInstance::new(vanilla_mob_effects::SPEED, 200));
        old_player
            .inventory
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::OAK_LOG, 4));
        *old_player.experience.lock() = Experience::from_parts(8, 0.25, 91);
        old_player.base.add_tag("carried".to_owned());
        let token = old_player
            .base
            .begin_pending_world_change()
            .expect("a live player can start a world change");

        let moved = old_player.moved_to_world(Arc::clone(target_world));

        assert!(Arc::ptr_eq(&moved.get_world(), target_world));
        assert!(Arc::ptr_eq(
            &moved.level().expect("the target world is alive"),
            target_world
        ));
        assert_eq!(moved.id(), old_player.id());
        assert_eq!(moved.generation(), old_player.generation());
        assert!(Arc::ptr_eq(&moved.session, &old_player.session));
        assert!(Arc::ptr_eq(&moved.inventory, &old_player.inventory));
        assert_eq!(moved.position(), DVec3::new(10.0, 80.0, 10.0));
        assert_eq!(moved.living().health().to_bits(), 7.0_f32.to_bits());
        assert_eq!(moved.food_data.lock().food_level, 5);
        assert!(moved.living().has_mob_effect(vanilla_mob_effects::SPEED));
        assert_eq!(moved.inventory.lock().get_item(0).count(), 4);
        assert_eq!(moved.experience.lock().total_points(), 91);
        assert_eq!(moved.tags(), ["carried"]);
        assert!(moved.is_world_change_token_pending(token));
        assert!(!moved.is_removed());

        assert!(old_player.is_removed());
        assert!(!old_player.is_world_change_pending());
        assert!(
            !old_player
                .living()
                .has_mob_effect(vanilla_mob_effects::SPEED)
        );
    }
}
