//! Thrown ender pearl projectile entity (`ThrownEnderpearl`).
//!
//! Mirrors vanilla `ThrownEnderpearl` (yarn `EnderPearlEntity`) on the Steel
//! `Projectile → ThrowableProjectile → ThrowableItemProjectile` trait stack.
//! On collision it teleports its owning player to the pearl's pre-move position,
//! deals 5.0 `ender_pearl` damage, plays the teleport sound, and discards itself.
//!
//! The pearl refreshes a timeout chunk ticket (`TicketType.ENDER_PEARL`) each tick
//! so it keeps flying across the simulation border, and is registered on its owning
//! player so it persists with them and re-spawns on login (vanilla
//! `ServerPlayer.enderPearls`).

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{RelativeMovement, SoundSource};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::EnderPearlEntityData;
use steel_registry::vanilla_game_rules::ENDER_PEARLS_VANISH_ON_DEATH;
use steel_registry::{sound_events, vanilla_damage_types, vanilla_items};
use steel_utils::ChunkPos;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::chunk::chunk_map::ENDER_PEARL_TICKET_TIMEOUT;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity, Projectile, ProjectileBase,
    ProjectileHit, RemovalReason, SharedEntity, ThrowableItemProjectile, ThrowableProjectile,
    change_entity_world,
};
use crate::player::Player;
use crate::portal::{TeleportPostTransition, TeleportTransition};
use crate::world::World;

/// Fall-style damage dealt to the teleporting owner (vanilla `enderPearl()`, 5.0).
const TELEPORT_DAMAGE: f32 = 5.0;

/// A thrown ender pearl.
#[entity_behavior(class = "ThrownEnderpearl")]
pub struct EnderPearlEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data carrying the rendered item stack.
    entity_data: SyncMutex<EnderPearlEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
    /// Countdown until the chunk-loading ticket is refreshed (vanilla `ticketTimer`).
    ticket_timer: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EnderPearlEntity`.
unsafe impl DowncastType for EnderPearlEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/ender_pearl");
}

impl EnderPearlEntity {
    /// Creates a new ender pearl with no owner and the default rendered item.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(EnderPearlEntityData::new()),
            projectile_base: ProjectileBase::new(),
            ticket_timer: SyncMutex::new(0),
        }
    }

    /// Creates an ender pearl from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(EnderPearlEntityData::new()),
            projectile_base: ProjectileBase::new(),
            ticket_timer: SyncMutex::new(0),
        }
    }

    /// Resolves the owner as an online player, including cached cross-world refs.
    fn owner_player(&self) -> Option<SharedEntity> {
        let owner = self.get_owner()?;
        owner.as_player()?;
        Some(owner)
    }

    /// Removes this pearl from its owner's persistence set when it hits or is
    /// discarded (vanilla `ThrownEnderpearl.onRemoval` deregistration).
    fn deregister_from_owner(&self) {
        if let Some(owner) = self.owner_player()
            && let Some(player) = owner.as_player()
        {
            player.deregister_ender_pearl(self.uuid());
        }
    }

    /// Refreshes the chunk-loading ticket so the pearl keeps flying across the
    /// simulation border (vanilla `ThrownEnderpearl.tick` →
    /// `registerAndUpdateEnderPearlTicket`).
    ///
    /// Re-places the ticket when the countdown lapses or the pearl crosses a chunk
    /// border, but only while owned by an online player.
    fn update_ender_pearl_ticket(&self, world: &Arc<World>) {
        let current_chunk = ChunkPos::from_entity_pos(self.position());
        let crossed_border = ChunkPos::from_entity_pos(self.old_position()) != current_chunk;

        let mut timer = self.ticket_timer.lock();
        *timer -= 1;
        if (*timer > 0 && !crossed_border) || self.owner_player().is_none() {
            return;
        }

        world.chunk_map.place_ender_pearl_ticket(current_chunk);
        // Vanilla `registerAndUpdateEnderPearlTicket` returns `timeout - 1`.
        *timer = ENDER_PEARL_TICKET_TIMEOUT as i32 - 1;
    }

    /// Vanilla `ThrownEnderpearl.tick` owner-death short-circuit: a pearl whose
    /// owner is a dead player vanishes when the gamerule is set.
    fn should_vanish_on_owner_death(&self, world: &Arc<World>) -> bool {
        let Some(owner) = self.owner_player() else {
            return false;
        };
        let Some(player) = owner.as_player() else {
            return false;
        };
        // Vanilla checks `!owner.isAlive()` for a dead (but still connected) player.
        // `LivingEntity::is_alive` is the health-based override vanilla dispatches to.
        Self::should_vanish_for_owner_state(
            LivingEntity::is_alive(player),
            player.has_won_game(),
            world.get_game_rule(&ENDER_PEARLS_VANISH_ON_DEATH),
        )
    }

    const fn should_vanish_for_owner_state(
        owner_alive: bool,
        owner_won_game: bool,
        vanish_on_death_rule: bool,
    ) -> bool {
        !owner_alive && !owner_won_game && vanish_on_death_rule
    }

    /// Vanilla `ThrownEnderpearl.isAllowedToTeleportOwner`.
    fn is_allowed_to_teleport_owner(world: &Arc<World>, player: &Player) -> bool {
        let player_world = player.get_world();
        if Arc::ptr_eq(&player_world, world) {
            return LivingEntity::is_alive(player) && !player.is_sleeping();
        }

        player.can_use_portal(true)
    }

    /// Teleports the owning player and applies the pearl's effects.
    ///
    /// Mirrors the `ServerPlayer` branch of vanilla `ThrownEnderpearl.onHit`.
    fn teleport_owner(
        &self,
        world: &Arc<World>,
        owner: &SharedEntity,
        player: &Player,
        teleport_pos: DVec3,
    ) {
        // TODO: 5% endermite spawn (Endermite entity not implemented).
        if self.is_on_portal_cooldown() {
            player.reset_portal_cooldown();
        }

        let transition = TeleportTransition {
            target_world: Arc::clone(world),
            position: teleport_pos,
            rotation: (0.0, 0.0),
            velocity: DVec3::ZERO,
            relatives: RelativeMovement::ROTATION.union(RelativeMovement::DELTA),
            portal_cooldown: player.portal_cooldown(),
            as_passenger: false,
            post_transition: TeleportPostTransition::do_nothing(),
        };
        let Some(new_owner) = change_entity_world(Arc::clone(owner), &transition) else {
            log::debug!("failed to teleport ender pearl owner {}", self.id());
            return;
        };
        let Some(new_player) = new_owner.as_player() else {
            return;
        };

        new_player.reset_fall_distance();
        new_player.reset_current_impulse_context();

        let damage = DamageSource::environment(&vanilla_damage_types::ENDER_PEARL);
        new_player.hurt(world, &damage, TELEPORT_DAMAGE);

        world.play_sound_at(
            &sound_events::ENTITY_PLAYER_TELEPORT,
            SoundSource::Players,
            teleport_pos,
            1.0,
            1.0,
            None,
        );
    }
}

impl Entity for EnderPearlEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        // Vanilla `ThrownEnderpearl.tick`: vanish if the owner died (gamerule),
        // otherwise run the throwable projectile movement/collision loop and keep
        // the pearl's chunk loaded via the ENDER_PEARL ticket.
        let Some(world) = self.level() else {
            self.throwable_projectile_tick();
            return;
        };

        if self.should_vanish_on_owner_death(&world) {
            self.deregister_from_owner();
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        self.throwable_projectile_tick();

        if self.is_alive() {
            self.update_ender_pearl_ticket(&world);
        }
    }

    fn get_default_gravity(&self) -> f64 {
        self.throwable_default_gravity()
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn spawn_data(&self) -> i32 {
        self.get_owner().map_or(0, |owner| owner.id())
    }

    fn restore_owner_reference(&self, owner: &SharedEntity) {
        self.cache_owner_entity(owner);
    }

    fn projectile_owner_uuid(&self) -> Option<uuid::Uuid> {
        self.owner_uuid()
    }

    fn projectile_owner(&self) -> Option<SharedEntity> {
        self.get_owner()
    }

    fn attackable(&self) -> bool {
        false
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        // Vanilla `Projectile.hurtServer` marks hurt but never takes damage.
        false
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        self.save_throwable_item(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        self.load_throwable_item(nbt);
    }
}

impl Projectile for EnderPearlEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn on_hit_entity(&self, entity: &SharedEntity, _location: DVec3) {
        // Vanilla `ThrownEnderpearl.onHitEntity`: deal 0 damage with a `thrown`
        // source so the hit entity registers the impact without being hurt.
        let mut damage =
            DamageSource::environment(&vanilla_damage_types::THROWN).with_direct_entity(self.id());
        if let Some(owner) = self.get_owner() {
            damage = damage.with_causing_entity(owner.id());
        }
        if let Some(world) = entity.level() {
            entity.hurt(&world, &damage, 0.0);
        }
    }

    fn on_hit(&self, hit: &ProjectileHit) {
        // Vanilla `ThrownEnderpearl.onHit`: super.onHit() then teleport the owner.
        self.projectile_on_hit(hit);

        // TODO: spawn 32 portal particles (needs CLevelParticles packet).
        let Some(world) = self.level() else {
            return;
        };
        if self.is_removed() {
            return;
        }

        let teleport_pos = self.old_position();
        if let Some(owner) = self.owner_player()
            && let Some(player) = owner.as_player()
            && Self::is_allowed_to_teleport_owner(&world, player)
        {
            self.teleport_owner(&world, &owner, player, teleport_pos);
        }
        self.deregister_from_owner();
        self.set_removed(RemovalReason::Discarded);
    }
}

impl ThrowableProjectile for EnderPearlEntity {}

impl ThrowableItemProjectile for EnderPearlEntity {
    fn get_default_item(&self) -> ItemRef {
        &vanilla_items::ENDER_PEARL
    }

    fn set_item(&self, item: ItemStack) {
        self.entity_data
            .lock()
            .throwable_item_projectile
            .item_stack
            .set(item);
    }

    fn get_item(&self) -> ItemStack {
        self.entity_data
            .lock()
            .throwable_item_projectile
            .item_stack
            .get()
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{test_support::init_test_registry, vanilla_entities, vanilla_items};

    use crate::entity::{Entity, Projectile, ThrowableItemProjectile};
    use crate::world::World;

    use super::EnderPearlEntity;

    #[test]
    fn shoot_aligns_velocity_with_direction() {
        init_test_registry();

        let pearl = EnderPearlEntity::new(
            &vanilla_entities::ENDER_PEARL,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        pearl.shoot(DVec3::new(0.0, 0.0, 1.0), 1.5, 1.0);

        let velocity = pearl.velocity();
        assert!(velocity.z > 0.0);
        assert!((velocity.length() - 1.5).abs() < 0.1);
        assert!(velocity.x.abs() < 0.1 && velocity.y.abs() < 0.1);
    }

    #[test]
    fn owner_round_trips() {
        init_test_registry();

        let pearl = EnderPearlEntity::new(
            &vanilla_entities::ENDER_PEARL,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        assert!(pearl.owner_uuid().is_none());

        let uuid = uuid::Uuid::from_u128(0x1234_5678_9abc_def0);
        pearl.set_owner_uuid(Some(uuid));
        assert_eq!(pearl.owner_uuid(), Some(uuid));
        assert_eq!(pearl.projectile_owner_uuid(), Some(uuid));
    }

    #[test]
    fn default_item_is_ender_pearl() {
        init_test_registry();

        let pearl = EnderPearlEntity::new(
            &vanilla_entities::ENDER_PEARL,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        assert_eq!(pearl.get_default_item().key, vanilla_items::ENDER_PEARL.key);
    }

    #[test]
    fn owner_death_vanish_predicate_respects_won_game() {
        assert!(EnderPearlEntity::should_vanish_for_owner_state(
            false, false, true
        ));
        assert!(!EnderPearlEntity::should_vanish_for_owner_state(
            false, true, true
        ));
        assert!(!EnderPearlEntity::should_vanish_for_owner_state(
            true, false, true
        ));
        assert!(!EnderPearlEntity::should_vanish_for_owner_state(
            false, false, false
        ));
    }
}
