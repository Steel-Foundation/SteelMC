//! Portals, portal cooldowns and world changes.

use super::*;

/// Portals, portal cooldowns and world changes.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityHooks`], never by implementing this trait.
pub trait EntityPortals: Entity {
    /// Default for [`Entity::relative_portal_position`].
    fn relative_portal_position_default(&self, axis: Axis, portal_area: FoundRectangle) -> DVec3 {
        PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        )
    }

    /// Runs vanilla `Entity.handlePortal` behavior currently implemented by Steel.
    fn handle_portal(&self) {
        self.base().process_portal_cooldown();
        let Some(world) = self.level() else {
            return;
        };
        let Some(process) = self.base().portal_process() else {
            return;
        };

        let player_invulnerable = self
            .as_player()
            .map(|player| player.abilities.lock().invulnerable);
        let transition_time = process
            .portal()
            .transition_time_for_player_state(&world, player_invulnerable);
        match self
            .base()
            .process_portal_teleportation(self.can_use_portal(false), transition_time)
        {
            Some(PortalProcessResult::Ready) => {
                let Some(pending_token) = self.begin_pending_world_change() else {
                    return;
                };
                let Some(entity) = world.get_entity_by_id(self.id()) else {
                    self.finish_pending_world_change(pending_token);
                    return;
                };
                self.reset_portal_cooldown();
                world.queue_world_change(
                    entity,
                    WorldChangeRequest::Portal {
                        portal: process.portal(),
                        source_world: Arc::clone(&world),
                        portal_pos: process.entry_position(),
                        pending_token,
                    },
                );
            }
            Some(PortalProcessResult::Waiting)
                if self
                    .base()
                    .portal_process()
                    .is_some_and(PortalProcessor::has_expired) =>
            {
                self.base().clear_portal_process();
            }
            Some(PortalProcessResult::Waiting) | None => {}
        }
    }

    /// Marks this entity as waiting for a prepared world change.
    fn begin_pending_world_change(&self) -> Option<PendingWorldChangeToken> {
        self.base().begin_pending_world_change()
    }

    /// Clears a pending world change if it still matches the provided token.
    fn finish_pending_world_change(&self, token: PendingWorldChangeToken) -> bool {
        self.base().finish_pending_world_change(token)
    }

    /// Returns true while this entity is waiting for a prepared world change.
    fn is_world_change_pending(&self) -> bool {
        self.base().is_world_change_pending()
    }

    /// Returns true if the given world-change token is still pending.
    fn is_world_change_token_pending(&self, token: PendingWorldChangeToken) -> bool {
        self.base().is_world_change_token_pending(token)
    }

    /// Default for [`Entity::can_use_portal`].
    fn can_use_portal_default(&self, ignore_passenger: bool) -> bool {
        let entity_type = self.entity_type();
        if entity_type == &vanilla_entities::FISHING_BOBBER
            || entity_type == &vanilla_entities::ENDER_DRAGON
            || entity_type == &vanilla_entities::WITHER
        {
            return false;
        }

        (ignore_passenger || !self.is_passenger()) && self.is_alive()
    }

    /// Default for [`Entity::dimension_changing_delay`].
    fn dimension_changing_delay_default(&self) -> i32 {
        // TODO: move to the projectile preset once projectiles are ported.
        if self.entity_type().is_projectile {
            return 2;
        }
        if let Some(first_passenger) = self.first_passenger()
            && first_passenger.as_player().is_some()
        {
            return first_passenger.dimension_changing_delay();
        }
        300
    }

    /// Default for [`Entity::on_teleported`].
    fn on_teleported_default(&self) {}

    /// Returns the vanilla portal cooldown in ticks.
    fn portal_cooldown(&self) -> i32 {
        self.base().portal_cooldown()
    }

    /// Sets the vanilla portal cooldown in ticks.
    fn set_portal_cooldown(&self, portal_cooldown: i32) {
        self.base().set_portal_cooldown(portal_cooldown);
    }

    /// Returns whether the entity is on vanilla portal cooldown.
    fn is_on_portal_cooldown(&self) -> bool {
        self.base().is_on_portal_cooldown()
    }

    /// Resets vanilla portal cooldown to this entity's dimension-changing delay.
    fn reset_portal_cooldown(&self) {
        self.set_portal_cooldown(self.dimension_changing_delay());
    }

    /// Marks this entity as inside a vanilla portal during the current tick.
    fn set_as_inside_portal(&self, portal: PortalKind, entry_position: BlockPos) {
        if self.is_on_portal_cooldown() {
            self.reset_portal_cooldown();
            return;
        }

        self.base().set_as_inside_portal(portal, entry_position);
    }
}

impl<T: Entity + ?Sized> EntityPortals for T {}
