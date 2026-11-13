//! Per-type entity hooks: the `Entity` methods entity types override.
//!
//! [`entity_hooks!`] declares each hook once, at the end of this file, and
//! expands into:
//!
//! - [`EntityProps<E>`]: one `fn(&E, ..)` per hook, typed for the concrete
//!   entity. `EntityProps::new()` holds vanilla `Entity`'s behavior; layer
//!   presets such as `living_entity::entity_props()` override fields with
//!   struct-update syntax.
//! - [`EntityHooks`]: the hooks as methods, a supertrait of [`Entity`]. It is
//!   implemented once, for every [`EntityTable`], by reading the type's `const`
//!   table. A call through `dyn Entity` is one virtual call, and the entry it
//!   lands on is known at compile time.
//!
//! Vanilla `Entity`'s body for a hook is the function of the same name in
//! [`crate::entity::entity`] (`entity::tick`); `EntityProps::new()` lists them.
//!
//! # Calling `super`
//!
//! A layer keeps each override as a function of its module named after the
//! hook, and its preset lists them. An override that extends its parent the
//! way a vanilla subclass calls `super` calls the parent layer's function,
//! which is `Entity`'s when no layer in between overrides the hook:
//!
//! ```ignore
//! // living_entity
//! pub fn tick<E: Parts>(living: &E) {
//!     entity::tick(living);
//!     // ...the rest of `LivingEntity.tick`
//! }
//!
//! // mob
//! pub fn tick<E: Parts>(mob: &E) {
//!     living_entity::tick(mob);
//!     // ...
//! }
//! ```
//!
//! A layer has a function only for the hooks its vanilla class overrides, so
//! `super` names the nearest ancestor that has one. A layer that gains an
//! override must repoint the subtypes that called past it.

use super::*;

/// Declares the entity hooks: generates [`EntityProps`] and [`EntityHooks`].
macro_rules! entity_hooks {
    (
        $(
            $(#[$meta:meta])*
            fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;
        )*
    ) => {
        /// Per-type entity hooks, typed for the concrete entity `E`.
        pub struct EntityProps<E: 'static> {
            $(
                $(#[$meta])*
                pub $name: fn(&E $(, $ty)*) $(-> $ret)?,
            )*
        }

        impl<E: Entity> EntityProps<E> {
            /// Every hook at vanilla `Entity`'s behavior.
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    $( $name: $name::<E>, )*
                }
            }
        }

        impl<E: Entity> Default for EntityProps<E> {
            fn default() -> Self {
                Self::new()
            }
        }

        /// The entity hooks as methods. Implemented for every
        /// [`EntityTable`]; entity types change them through their table.
        pub trait EntityHooks {
            $(
                $(#[$meta])*
                fn $name(&self $(, $arg: $ty)*) $(-> $ret)?;
            )*
        }

        impl<T: EntityTable> EntityHooks for T {
            $(
                fn $name(&self $(, $arg: $ty)*) $(-> $ret)? {
                    (T::ENTITY.$name)(self $(, $arg)*)
                }
            )*
        }
    };
}

/// A concrete entity type's hook table.
///
/// Every entity type implements it, like its layer tables; an empty impl runs
/// vanilla `Entity`'s behavior.
pub trait EntityTable: Entity + Sized {
    /// The hooks this type runs.
    const ENTITY: EntityProps<Self> = EntityProps::new();
}

entity_hooks! {
    /// Returns whether this entity ignores chunk ticking visibility.
    fn is_always_ticking() -> bool;
    /// Returns whether this entity should be broadcast to the given player.
    ///
    /// Mirrors vanilla `Entity.broadcastToPlayer`. Most entities are always
    /// broadcastable; players override this for spectator visibility rules.
    fn broadcast_to_player(player: &Player) -> bool;
    /// Returns this entity's vanilla scoreboard holder name.
    ///
    /// Non-player entities use their UUID string. Players override this with
    /// their game profile name.
    fn scoreboard_name() -> String;
    /// Returns this entity's vanilla name component before team decoration.
    fn name() -> TextComponent;
    /// Returns this entity's vanilla display name.
    fn display_name() -> TextComponent;
    /// Returns this entity's plain vanilla name.
    ///
    /// Custom names are resolved as text components; otherwise entities use
    /// their translated type description. Players override this with their
    /// game profile name.
    fn plain_text_name() -> String;
    /// Returns whether this entity obstructs block placement.
    ///
    /// Base entities do not obstruct placement unless a concrete entity type opts in.
    fn blocks_building() -> bool;
    /// Returns whether this entity can be targeted by picking and interaction raycasts.
    ///
    /// Base entities are not pickable unless a concrete entity type opts in.
    fn is_pickable() -> bool;
    /// Returns whether this entity can be attacked.
    ///
    /// Entity types that reject player attacks set this to `false`.
    fn attackable() -> bool;
    /// Returns whether this entity participates in vanilla push separation.
    ///
    /// Base entities are not pushable unless a concrete entity type opts in.
    fn is_pushable() -> bool;
    /// Returns whether vanilla fluid currents can push this entity.
    ///
    /// The extractor flattens vanilla's per-subclass overrides into one flag.
    /// Entities with extra conditions set this hook, as `Player` does.
    fn is_pushed_by_fluid() -> bool;
    /// Applies bubble-column surface push velocity, dispatching to the
    /// projectile override when this entity is a projectile.
    fn on_above_bubble_column(
        drag_down: bool,
        pos: BlockPos,
    );
    /// Applies bubble-column push velocity while submerged in the column,
    /// dispatching to the projectile override when this entity is a projectile.
    fn on_inside_bubble_column(drag_down: bool);
    /// Returns whether this entity is invisible to normal entity selectors.
    ///
    /// Base entities are never spectators; players override this from their game mode.
    fn is_spectator() -> bool;
    /// Returns the living entity credited as this explosion's indirect source.
    fn explosion_indirect_source() -> Option<SharedEntity>;
    /// Returns the point used to calculate this entity's explosion direction.
    fn explosion_damage_origin() -> DVec3;
    /// Returns whether this entity is wholly ignored by an explosion.
    fn ignore_explosion(explosion: &dyn Explosion) -> bool;
    /// Lets an entity source modify effective block or fluid resistance.
    fn block_explosion_resistance(
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
        resistance: f32,
    ) -> f32;
    /// Lets an entity source veto one affected block position.
    fn should_block_explode(
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool;
    /// Called after this entity is processed by an explosion.
    fn on_explosion_hit(
        explosion_source: Option<&dyn Entity>,
    );
    /// Returns whether `other` can collide with this entity.
    ///
    /// Base entities cannot be collided with unless a concrete entity type opts in.
    fn can_be_collided_with(
        other: Option<&dyn Entity>,
    ) -> bool;
    /// Whether movement queries must consider this entity for hard collisions.
    ///
    /// Returning `false` promises [`EntityHooks::can_be_collided_with`] is always false and
    /// [`EntityHooks::can_collide_with`] never broadens the default rule. This capability may
    /// be cached for the registered lifetime, including chunk-unload retention.
    fn is_hard_collision_relevant() -> bool;
    /// Returns whether projectile collision may interact with this entity.
    fn can_be_hit_by_projectile() -> bool;
    /// Starts riding `entity_to_ride` if vanilla boarding rules allow it.
    fn start_riding(entity_to_ride: &SharedEntity) -> bool;
    /// Returns whether this entity can accept `passenger` as a direct passenger.
    fn can_add_passenger(passenger: &dyn Entity) -> bool;
    /// Returns whether this entity can collide with `other`.
    fn can_collide_with(other: &dyn Entity) -> bool;
    /// Applies vanilla entity-to-entity push separation.
    fn push_entity(entity: &dyn Entity);
    /// Runs entity-specific behavior after falling below the world.
    fn on_below_world();
    /// Runs vanilla pre-tick despawn checks.
    fn check_despawn();
    /// Applies an inside-block effect queued by vanilla's step-based collector.
    fn apply_inside_block_effect(
        effect_type: InsideBlockEffectType,
    );
    /// Updates synchronized entity data just before tracker sync.
    fn update_data_before_sync();
    /// Returns whether this entity is alive for vanilla generic entity checks.
    fn is_alive() -> bool;
    /// Called after a teleport completes successfully.
    fn on_teleported();
    /// Called by leashables while this entity is their live leash holder.
    fn notify_leash_holder(leashable: &dyn Entity);
    /// Called when a leashable stops using this entity as its live leash holder.
    fn notify_leashee_removed(leashable: &dyn Entity);
    /// Visits vanilla `Entity.getWeaponItem` without copying inventory state.
    fn with_weapon_item(
        visitor: &mut dyn FnMut(Option<&ItemStack>),
    );
    /// Returns true when movement is driven by serverbound movement packets.
    fn uses_client_movement_packets() -> bool;
    /// Returns true when vanilla allows this side to apply movement simulation side
    /// effects.
    fn can_simulate_movement() -> bool;
    /// Returns true when vanilla allows this side to run entity AI/travel logic.
    fn is_effective_ai() -> bool;
    /// Returns true when vanilla landing bounce should be suppressed.
    fn is_suppressing_bounce() -> bool;
    /// Returns true when vanilla block step-on hooks should treat this entity as
    /// careful.
    fn is_stepping_carefully() -> bool;
    /// Returns true if vanilla rules consider this entity to be on a climbable block.
    fn on_climbable() -> bool;
    /// Returns the movement vector vanilla exposes for block-contact logic.
    fn known_movement() -> DVec3;
    /// Returns the base-tick displacement vanilla exposes as `getKnownSpeed`.
    fn known_speed() -> DVec3;
    /// Returns vanilla small and big fall sounds for this entity.
    fn fall_sounds() -> (SoundEventRef, SoundEventRef);
    /// Rotates this entity to face a fixed position.
    fn look_at(from_anchor: EntityAnchor, target: DVec3);
    /// Rotates this entity to follow an anchored point on another entity.
    fn look_at_entity(
        from_anchor: EntityAnchor,
        target: &dyn Entity,
        target_anchor: EntityAnchor,
    );
    /// Extra spawn-packet data used by vanilla for entity-specific construction.
    fn spawn_data() -> i32;
    /// Returns whether this entity is immune to fire effects and fire damage.
    fn fire_immune() -> bool;
    /// Returns vanilla fire immunity cooldown ticks after not being ignited.
    fn fire_immune_ticks() -> i32;
    /// Returns whether vanilla should play this entity's lava hurt sound.
    fn should_play_lava_hurt_sound() -> bool;
    /// Maximum ticks this entity's remaining-fire timer can hold; `None` means
    /// unlimited.
    fn remaining_fire_ticks_cap() -> Option<i32>;
    /// Returns extra ticks added by fire-block ignition before 8-second ignition.
    fn fire_ignite_extra_ticks() -> i32;
    /// Moves this entity to `pos`, keeping its current rotation.
    fn teleport_to(pos: DVec3) -> Result<(), EntityMoveError>;
    /// Returns the block position used for movement-affecting block properties.
    fn block_pos_below_that_affects_movement() -> Option<BlockPos>;
    /// Returns vanilla dimensions for a physical pose.
    fn dimensions_for_pose(
        pose: EntityPose,
    ) -> EntityDimensions;
    /// Returns whether vanilla currently considers this entity crouching.
    fn is_crouching() -> bool;
    /// Returns whether vanilla currently considers this entity swimming.
    fn is_swimming() -> bool;
    /// Returns vanilla movement side effects emitted by this entity.
    fn movement_emission() -> EntityMovementEmission;
    /// Returns whether this entity may modify the world at a position.
    ///
    /// Vanilla `Entity.mayInteract` defaults to true; players apply world
    /// permission checks.
    fn may_interact(world: &World, pos: BlockPos) -> bool;
    /// Returns this entity's vanilla sound source category.
    fn sound_source() -> SoundSource;
    /// Returns this entity's vanilla swim sound.
    fn swim_sound() -> SoundEventRef;
    /// Plays an entity sound at the entity's exact position.
    fn play_sound(sound: SoundEventRef, volume: f32, pitch: f32);
    /// Returns the sound played when this entity enters water at low speed.
    fn swim_splash_sound() -> SoundEventRef;
    /// Returns the sound played when this entity enters water at high speed.
    fn swim_high_speed_splash_sound() -> SoundEventRef;
    /// Plays the base vanilla step sound for a block.
    fn play_step_sound(
        pos: BlockPos,
        block_state: BlockStateId,
    );
    /// Returns whether the entity is currently flapping.
    fn is_flapping() -> bool;
    /// Runs entity-specific flap side effects.
    fn on_flap();
    /// Maximum height this entity can step up during normal movement.
    fn max_up_step() -> f32;
    /// Whether movement should apply player-style sneak edge prevention.
    fn backs_off_from_edge() -> bool;
    /// Gets the default gravity for this entity type.
    ///
    /// Vanilla values: `ItemEntity` = 0.04, `LivingEntity` = 0.08
    fn default_gravity() -> f64;
    /// Applies entity-specific implicit components carried by an item stack.
    ///
    /// Mirrors the overridable part of vanilla's `Entity.applyImplicitComponents`.
    fn apply_implicit_item_components(
        item_stack: &ItemStack,
    );
    /// Runs when this entity causes another entity to die.
    /// The entity provided is the entity who was killed.
    fn killed_entity(
        world: &World,
        entity: &Living<'_>,
        source: &DamageSource,
    ) -> bool;
    /// Runs when this entity kills another entity.
    fn award_kill_score(
        victim: &dyn Entity,
        killing_blow: &DamageSource,
    );
    /// Returns whether this entity is excluded from pressure plates and tripwires.
    ///
    /// Display-like entities and marker entities override this capability.
    fn is_ignoring_block_triggers() -> bool;
    /// Returns how vanilla lets this entity respond to piston movement.
    fn piston_push_reaction() -> PushReaction;
    /// Returns whether this entity can accept any passenger.
    fn could_accept_passenger() -> bool;
    /// Returns whether this entity handles and consumes an attack before normal damage.
    fn skip_attack_interaction(source: &SharedEntity) -> bool;
    /// Caches a live owner reference after restoring persisted owner-linked
    /// entities. Most entities do not store owner references.
    fn restore_owner_reference(owner: &SharedEntity);
    /// Gets the position to send in this entity's add-entity packet.
    ///
    /// Mirrors vanilla `Entity.getAddEntityPacket()` overloads. Most entities
    /// spawn at their current position; block-attached entities can override
    /// this with the backing block position used by the vanilla packet.
    fn spawn_position() -> DVec3;
    /// Refreshes dimensions for the current physical pose.
    fn refresh_dimensions();
    /// Adds an impulse to this entity's velocity and marks velocity for sync.
    fn push_impulse(impulse: DVec3);
    /// Attempts to set the entity's position through world lifecycle validation.
    ///
    /// For the base method, see [`Self::try_set_position_base`]
    fn try_set_position(pos: DVec3) -> Result<(), EntityMoveError>;
    /// `Entity.onRemoval`: runs once, after the entity is first removed.
    fn on_removal(reason: RemovalReason);
    /// Called when a player touches this entity during nearby pickup processing.
    fn player_touch(player: &Arc<Player>);
    /// Kills this entity. `world` is vanilla's explicit `ServerLevel`
    /// argument; death game events use the entity's attached world.
    fn kill(world: &World);
    /// Returns whether this entity may enter a portal.
    fn can_use_portal(ignore_passenger: bool) -> bool;
    /// Returns the portal cooldown, in ticks, applied after changing dimension.
    fn dimension_changing_delay() -> i32;
    /// Returns this entity's position relative to the portal, in portal-local space.
    fn relative_portal_position(
        axis: Axis,
        portal_area: FoundRectangle,
    ) -> DVec3;
    /// Returns this entity's head yaw.
    fn y_head_rot() -> f32;
    /// Sets this entity's head yaw.
    fn set_y_head_rot(y_head_rot: f32);
    /// Returns whether the entity is currently suffocating inside a solid block.
    fn is_in_wall() -> bool;
    /// Returns the horizontal air drag applied after movement.
    fn air_drag() -> f32;
    /// Returns the gravity applied this tick.
    fn effective_gravity() -> f64;
    /// Returns whether this entity rides on rails, for movement emission.
    fn is_on_rails() -> bool;
    /// Returns the horizontal direction this entity moves in.
    fn motion_direction() -> Direction;
    /// Returns the vehicle-relative point where `passenger` attaches.
    fn passenger_attachment_point(passenger: &dyn Entity) -> DVec3;
    /// Where `passenger` stands after leaving this vehicle; may set
    /// the passenger's pose to the one that fits there.
    fn dismount_location_for_passenger(passenger: &Living<'_>) -> DVec3;
    /// Moves this entity by `delta`, resolving collisions.
    fn move_entity(mover_type: MoverType, delta: DVec3) -> Option<MoveResult>;
    /// Vanilla `Entity.tick`, called every game tick while the entity is
    /// in a ticked chunk. The caller handles post-tick dirty data sync.
    fn tick();
    /// Vanilla `Entity.baseTick`. It stays separate from
    /// [`EntityHooks::tick`] because several vanilla subclasses override
    /// `tick` without calling `super.tick()`.
    fn base_tick();
    /// Vanilla `Entity.hurtServer`: applies damage, returning whether it
    /// was taken.
    fn hurt(world: &World, source: &DamageSource, amount: f32) -> bool;
    /// Vanilla `Entity.rideTick`: a passenger's tick.
    fn ride_tick();
    /// Vanilla `Entity.checkFallDamage`: accumulates fall distance, lands
    /// on the block fallen on and resets it.
    fn check_fall_damage(
        vertical_movement: f64,
        on_ground: bool,
        on_state: BlockStateId,
        pos: BlockPos,
        world: &Arc<World>,
    );
    /// Vanilla `Entity.causeFallDamage`.
    fn cause_fall_damage(
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool;
    /// Vanilla `Entity.canFreeze`.
    fn can_freeze() -> bool;
    /// Vanilla `Entity.makeStuckInBlock`.
    fn make_stuck_in_block(
        state: BlockStateId,
        speed_multiplier: DVec3,
    );
    /// Vanilla `Entity.doWaterSplashEffect`.
    fn do_water_splash_effect();
    /// Vanilla `Entity.updateSwimming`.
    fn update_swimming();
    /// Vanilla `Entity.stopRiding`.
    fn stop_riding();
    /// Vanilla `Entity.removeVehicle`: leaves the vehicle without
    /// placing the entity.
    fn remove_vehicle();
    /// Vanilla `Entity.getBlockSpeedFactor`.
    fn block_speed_factor() -> f32;
    /// Vanilla `Entity.interact`.
    fn interact(
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult;
    /// Vanilla `Entity.setYBodyRot`.
    fn set_y_body_rot(y_body_rot: f32);
    /// Vanilla `Entity.positionRider`: places a direct passenger on this
    /// vehicle.
    fn position_rider(passenger: &dyn Entity);
    /// Vanilla `Entity.getControllingPassenger`: the passenger steering
    /// this entity.
    fn controlling_passenger() -> Option<SharedEntity>;
}
