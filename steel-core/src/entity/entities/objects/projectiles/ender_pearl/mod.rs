//! Vanilla `ThrownEnderpearl`.
//!
//! The pearl keeps its chunk loaded with a timed ticket while it flies and is
//! registered on its owning player, so it is saved and respawned with them.

use crate::entity::ProjectileHit;
use crate::entity::entity;
use crate::entity::projectile;
use crate::entity::projectile::ProjectileTable;
use std::sync::Arc;

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{RelativeMovement, SoundSource};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::EnderPearlEntityData;
use steel_registry::vanilla_game_rules::{
    ENDER_PEARLS_VANISH_ON_DEATH, SPAWN_MOBS, SPAWN_MONSTERS,
};
use steel_registry::{sound_events, vanilla_damage_types, vanilla_entities, vanilla_items};
use steel_utils::ChunkPos;
use steel_utils::locks::CheckedMutex;
use steel_utils::types::Difficulty;

use crate::chunk::chunk_map::ENDER_PEARL_TICKET_TIMEOUT;
use crate::entity::EntityTable;
use crate::entity::damage::DamageSource;
use crate::entity::entities::EndermiteEntity;
use crate::entity::prelude::*;
use crate::entity::projectile::throwable_item::ThrownItem;
use crate::entity::projectile::{ProjectileBase, ProjectileProps, throwable};
use crate::entity::{
    Entity, EntityBase, EntityProps, RemovalReason, SharedEntity, change_entity_world,
    next_entity_id,
};
use crate::player::Player;
use crate::portal::{TeleportPostTransition, TeleportTransition};
use crate::world::World;

/// A thrown ender pearl.
#[entity_behavior(class = "ThrownEnderpearl")]
#[derive(Entity)]
#[entity(key = "steel:entity/ender_pearl", constructors)]
pub struct EnderPearlEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    #[entity(component)]
    thrown_item: ThrownItem,
    /// Ticks until the chunk ticket is renewed (vanilla `ticketTimer`).
    ticket_timer: CheckedMutex<i64>,
    #[entity(synced)]
    entity_data: CheckedMutex<EnderPearlEntityData>,
}

impl EntityTable for EnderPearlEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: Self::entity_tick,
        on_removal: |pearl, reason| {
            // Pearls saved with their player stay registered for the next login.
            if reason != RemovalReason::StoredWithPlayer {
                pearl.deregister_from_owner();
            }
        },
        // Unlike other projectiles, pearls move through bubble columns like
        // any entity.
        on_above_bubble_column: |pearl, drag_down, pos| {
            entity::on_above_bubble_column(pearl, drag_down, pos);
        },
        on_inside_bubble_column: |pearl, drag_down| {
            entity::on_inside_bubble_column(pearl, drag_down);
        },
        ..throwable::entity_props()
    };
}

impl EnderPearlEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = EnderPearlEntityData::new();
        let thrown_item = ThrownItem::new(&vanilla_items::ENDER_PEARL, &mut entity_data);
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(),
            thrown_item,
            ticket_timer: CheckedMutex::new(0),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// The owner, when it is a player.
    fn owner_player(&self) -> Option<SharedEntity> {
        self.projectile()
            .owner()
            .filter(|owner| owner.as_player().is_some())
    }

    fn deregister_from_owner(&self) {
        if let Some(owner) = self.owner_player()
            && let Some(player) = owner.as_player()
        {
            player.deregister_ender_pearl(self);
        }
    }

    /// `ThrownEnderpearl.tick`: vanishes when its owner died (if the game
    /// rule says so), then flies and keeps its chunk loaded.
    fn entity_tick(&self) {
        let Some(world) = self.level() else {
            throwable::tick(self);
            return;
        };

        let previous_chunk = ChunkPos::from_entity_pos(self.position());
        let owner = self.owner_player();
        let owner_player = owner.as_ref().and_then(|owner| owner.as_player());
        if owner_player.is_some_and(|player| {
            !player.is_alive()
                && !player.has_won_game()
                && world.get_game_rule(&ENDER_PEARLS_VANISH_ON_DEATH)
        }) {
            self.set_removed(RemovalReason::Discarded);
        } else {
            throwable::tick(self);
        }

        if !self.is_alive() {
            return;
        }
        let current_chunk = ChunkPos::from_entity_pos(self.position());
        let renew = {
            let mut timer = self.ticket_timer.lock();
            *timer -= 1;
            *timer <= 0 || current_chunk != previous_chunk
        };
        if renew
            && let Some(player) = owner_player
            && let Some(pearl) = self.shared()
        {
            player.register_ender_pearl(&pearl);
            world.chunk_map.place_ender_pearl_ticket(current_chunk);
            // Vanilla `registerAndUpdateEnderPearlTicket` returns `timeout - 1`.
            *self.ticket_timer.lock() = ENDER_PEARL_TICKET_TIMEOUT - 1;
        }
    }

    /// `ThrownEnderpearl.onHit`: teleports the owner to where the pearl was
    /// before this tick's move, then disappears.
    fn projectile_on_hit(&self, hit: &ProjectileHit) {
        projectile::on_hit(self, hit);
        let projectile = self.projectile();
        // VANILLA CLIENT-LOCAL: the portal particles.
        let Some(world) = self.level() else {
            return;
        };
        if self.is_removed() {
            return;
        }

        if let Some(owner) = projectile.owner()
            && Self::is_allowed_to_teleport_owner(&owner, &world)
        {
            let teleport_pos = self.old_position();
            match owner.as_player() {
                Some(player) => self.teleport_player(&world, &owner, player, teleport_pos),
                None => Self::teleport_entity(&world, &owner, teleport_pos),
            }
        }
        self.set_removed(RemovalReason::Discarded);
    }

    /// The owner may be teleported when it is alive and not sleeping in this
    /// world, or may use portals when it is in another.
    fn is_allowed_to_teleport_owner(owner: &SharedEntity, world: &Arc<World>) -> bool {
        let same_world = owner
            .level()
            .is_some_and(|owner_world| Arc::ptr_eq(&owner_world, world));
        if !same_world {
            return owner.can_use_portal(true);
        }
        match owner.as_living_entity() {
            Some(living) => living.is_alive() && !living.is_sleeping(),
            None => owner.is_alive(),
        }
    }

    fn teleport_player(
        &self,
        world: &Arc<World>,
        owner: &SharedEntity,
        player: &Player,
        teleport_pos: DVec3,
    ) {
        // TODO: skip this while the player's connection is not accepting
        // messages (vanilla `connection.isAcceptingMessages`).
        if rand::random::<f32>() < 0.05
            && world.get_game_rule(&SPAWN_MOBS)
            && world.get_game_rule(&SPAWN_MONSTERS)
            && world.difficulty() != Difficulty::Peaceful
        {
            let endermite = EndermiteEntity::new(
                &vanilla_entities::ENDERMITE,
                next_entity_id(),
                owner.position(),
                Arc::downgrade(world),
            );
            endermite.set_rotation(owner.rotation());
            if let Err(error) = world.try_add_entity(endermite) {
                log::debug!("failed to add ender pearl endermite: {error}");
            }
        }
        if self.is_on_portal_cooldown() {
            owner.reset_portal_cooldown();
        }

        let transition = TeleportTransition {
            target_world: Arc::clone(world),
            position: teleport_pos,
            rotation: (0.0, 0.0),
            velocity: DVec3::ZERO,
            relatives: RelativeMovement::ROTATION,
            portal_cooldown: player.portal_cooldown(),
            as_passenger: false,
            post_transition: TeleportPostTransition::do_nothing(),
        };
        if let Some(new_owner) = change_entity_world(Arc::clone(owner), &transition) {
            new_owner.reset_fall_distance();
            if let Some(living) = new_owner.as_living_entity() {
                living.reset_current_impulse_context();
            }
            let damage = DamageSource::environment(&vanilla_damage_types::ENDER_PEARL);
            new_owner.hurt(world, &damage, 5.0);
        }
        Self::play_teleport_sound(world, teleport_pos);
    }

    fn teleport_entity(world: &Arc<World>, owner: &SharedEntity, teleport_pos: DVec3) {
        let transition = TeleportTransition {
            target_world: Arc::clone(world),
            position: teleport_pos,
            rotation: owner.rotation(),
            velocity: DVec3::ZERO,
            relatives: RelativeMovement::NONE,
            portal_cooldown: owner.portal_cooldown(),
            as_passenger: false,
            post_transition: TeleportPostTransition::do_nothing(),
        };
        if let Some(new_owner) = change_entity_world(Arc::clone(owner), &transition) {
            new_owner.reset_fall_distance();
            if let Some(living) = new_owner.as_living_entity() {
                living.reset_current_impulse_context();
            }
        }
        Self::play_teleport_sound(world, teleport_pos);
    }

    fn play_teleport_sound(world: &World, position: DVec3) {
        world.play_sound_at(
            &sound_events::ENTITY_PLAYER_TELEPORT,
            SoundSource::Players,
            position,
            1.0,
            1.0,
            None,
        );
    }
}

impl ProjectileTable for EnderPearlEntity {
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps {
        on_hit: Self::projectile_on_hit,
        on_hit_entity: |pearl, target, location| {
            projectile::on_hit_entity(pearl, target, location);
            if let Some(world) = target.level()
                && let Some(source) = pearl.projectile().thrown_damage()
            {
                target.hurt(&world, &source, 0.0);
            }
        },
        ..ProjectileProps::new()
    };
}

#[cfg(test)]
mod tests;
