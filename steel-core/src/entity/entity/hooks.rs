//! Per-type entity hooks: the `Entity` methods entity types override.
//!
//! [`entity_hook_list!`] declares each hook once. Callbacks expand it into:
//!
//! - [`EntityProps<E>`]: one `fn(&E, ..)` per hook, typed for the concrete
//!   entity. `EntityProps::new()` holds the defaults; layer presets such as
//!   `living_entity::entity_props()` override fields with struct-update syntax.
//! - [`EntityHooks`]: the same hooks on `dyn Entity`, implemented once for every
//!   `EntityProps<E>` and by [`EntityDefaults`].
//! - `final` dispatch methods on [`Entity`], so callers never see the plumbing.
//!
//! `ref` hooks take `&self`. `shared` hooks take `self: Arc<Self>` on [`Entity`]
//! and receive the [`SharedEntity`] handle next to the typed entity.
//!
//! Each hook names the `final` [`Entity`] method holding its default body.

use super::*;
use steel_utils::DowncastType;

/// Declares the entity hooks and passes them to `$callback`.
macro_rules! entity_hook_list {
    ($callback:ident) => {
        $callback! {
            ref {
                /// Returns whether this entity ignores chunk ticking visibility.
                fn is_always_ticking() -> bool = is_always_ticking_default;
                /// Returns whether this entity should be broadcast to the given player.
                ///
                /// Mirrors vanilla `Entity.broadcastToPlayer`. Most entities are always
                /// broadcastable; players override this for spectator visibility rules.
                fn broadcast_to_player(player: &Player) -> bool = broadcast_to_player_default;
                /// Returns this entity's vanilla scoreboard holder name.
                ///
                /// Non-player entities use their UUID string. Players override this with
                /// their game profile name.
                fn scoreboard_name() -> String = scoreboard_name_default;
                /// Returns this entity's vanilla name component before team decoration.
                fn name() -> TextComponent = name_default;
                /// Returns this entity's vanilla display name.
                fn display_name() -> TextComponent = display_name_default;
                /// Returns this entity's plain vanilla name.
                ///
                /// Custom names are resolved as text components; otherwise entities use
                /// their translated type description. Players override this with their
                /// game profile name.
                fn plain_text_name() -> String = plain_text_name_default;
                /// Returns whether this entity obstructs block placement.
                ///
                /// Base entities do not obstruct placement unless a concrete entity type opts in.
                fn blocks_building() -> bool = blocks_building_default;
                /// Returns whether this entity can be targeted by picking and interaction raycasts.
                ///
                /// Base entities are not pickable unless a concrete entity type opts in.
                fn is_pickable() -> bool = is_pickable_default;
                /// Returns whether this entity can be attacked.
                ///
                /// Entity types that reject player attacks set this to `false`.
                fn attackable() -> bool = attackable_default;
                /// Returns whether this entity participates in vanilla push separation.
                ///
                /// Base entities are not pushable unless a concrete entity type opts in.
                fn is_pushable() -> bool = is_pushable_default;
                /// Returns whether vanilla fluid currents can push this entity.
                ///
                /// The extractor flattens vanilla's per-subclass overrides into one flag.
                /// Entities with extra conditions set this hook, as `Player` does.
                fn is_pushed_by_fluid() -> bool = is_pushed_by_fluid_default;
                /// Applies bubble-column surface push velocity, dispatching to the
                /// projectile override when this entity is a projectile.
                fn on_above_bubble_column(
                    drag_down: bool,
                    pos: BlockPos,
                ) = on_above_bubble_column_default;
                /// Applies bubble-column push velocity while submerged in the column,
                /// dispatching to the projectile override when this entity is a projectile.
                fn on_inside_bubble_column(drag_down: bool) = on_inside_bubble_column_default;
                /// Returns whether this entity is invisible to normal entity selectors.
                ///
                /// Base entities are never spectators; players override this from their game mode.
                fn is_spectator() -> bool = is_spectator_default;
                /// Returns the living entity credited as this explosion's indirect source.
                fn explosion_indirect_source(
                    ,
                ) -> Option<SharedEntity> = explosion_indirect_source_default;
                /// Returns the point used to calculate this entity's explosion direction.
                fn explosion_damage_origin() -> DVec3 = explosion_damage_origin_default;
                /// Returns whether this entity is wholly ignored by an explosion.
                fn ignore_explosion(explosion: &dyn Explosion) -> bool = ignore_explosion_default;
                /// Lets an entity source modify effective block or fluid resistance.
                fn block_explosion_resistance(
                    explosion: &dyn Explosion,
                    world: &World,
                    pos: BlockPos,
                    state: BlockStateId,
                    fluid: FluidState,
                    resistance: f32,
                ) -> f32 = block_explosion_resistance_default;
                /// Lets an entity source veto one affected block position.
                fn should_block_explode(
                    explosion: &dyn Explosion,
                    world: &World,
                    pos: BlockPos,
                    state: BlockStateId,
                    power: f32,
                ) -> bool = should_block_explode_default;
                /// Called after this entity is processed by an explosion.
                fn on_explosion_hit(
                    explosion_source: Option<&dyn Entity>,
                ) = on_explosion_hit_default;
                /// Returns the vanilla `Projectile` owner UUID when this entity exposes one.
                fn projectile_owner_uuid() -> Option<Uuid> = projectile_owner_uuid_default;
                /// Returns true for vanilla players whose abilities have `flying` set.
                fn is_flying_player() -> bool = is_flying_player_default;
                /// Returns whether `other` can collide with this entity.
                ///
                /// Base entities cannot be collided with unless a concrete entity type opts in.
                fn can_be_collided_with(
                    other: Option<&dyn Entity>,
                ) -> bool = can_be_collided_with_default;
                /// Whether movement queries must consider this entity for hard collisions.
                ///
                /// Returning `false` promises [`Self::can_be_collided_with`] is always false and
                /// [`Self::can_collide_with`] never broadens the default rule. This capability may
                /// be cached for the registered lifetime, including chunk-unload retention.
                fn is_hard_collision_relevant() -> bool = is_hard_collision_relevant_default;
                /// Returns whether projectile collision may interact with this entity.
                fn can_be_hit_by_projectile() -> bool = can_be_hit_by_projectile_default;
                /// Stops riding the current vehicle, if any.
                fn stop_riding() = stop_riding_default;
                /// Starts riding `entity_to_ride` if vanilla boarding rules allow it.
                fn start_riding(entity_to_ride: &SharedEntity) -> bool = start_riding_default;
                /// Returns the living passenger currently controlling this entity, if any.
                ///
                /// Base entities have no controller; controllable vehicles override this
                /// based on their own rules.
                fn controlling_passenger() -> Option<SharedEntity> = controlling_passenger_default;
                /// Returns whether this entity can accept `passenger` as a direct passenger.
                fn can_add_passenger(passenger: &dyn Entity) -> bool = can_add_passenger_default;
                /// Repositions a direct passenger from this vehicle's attachment point.
                fn position_rider(passenger: &dyn Entity) = position_rider_default;
                /// Returns whether this entity can collide with `other`.
                fn can_collide_with(other: &dyn Entity) -> bool = can_collide_with_default;
                /// Applies vanilla entity-to-entity push separation.
                fn push_entity(entity: &dyn Entity) = push_entity_default;
                /// Runs vanilla `Entity.baseTick` pieces Steel currently implements.
                ///
                /// This intentionally stays separate from `tick()` because several vanilla
                /// subclasses override tick without calling `super.tick()`.
                fn base_tick() = base_tick_default;
                /// Runs entity-specific behavior after falling below the world.
                fn on_below_world() = on_below_world_default;
                /// Runs vanilla pre-tick despawn checks.
                fn check_despawn() = check_despawn_default;
                /// Applies an inside-block effect queued by vanilla's step-based collector.
                fn apply_inside_block_effect(
                    effect_type: InsideBlockEffectType,
                ) = apply_inside_block_effect_default;
                /// Updates synchronized entity data just before tracker sync.
                fn update_data_before_sync() = update_data_before_sync_default;
                /// Returns whether this entity is alive for vanilla generic entity checks.
                fn is_alive() -> bool = is_alive_default;
                /// Called after a teleport completes successfully.
                fn on_teleported() = on_teleported_default;
                /// Called by leashables while this entity is their live leash holder.
                fn notify_leash_holder(leashable: &dyn Entity) = notify_leash_holder_default;
                /// Called when a leashable stops using this entity as its live leash holder.
                fn notify_leashee_removed(leashable: &dyn Entity) = notify_leashee_removed_default;
                /// Visits vanilla `Entity.getWeaponItem` without copying inventory state.
                fn with_weapon_item(
                    visitor: &mut dyn FnMut(Option<&ItemStack>),
                ) = with_weapon_item_default;
                /// Returns true when movement is driven by serverbound movement packets.
                fn uses_client_movement_packets() -> bool = uses_client_movement_packets_default;
                /// Returns true when vanilla allows this side to apply movement simulation side
                /// effects.
                fn can_simulate_movement() -> bool = can_simulate_movement_default;
                /// Returns true when vanilla allows this side to run entity AI/travel logic.
                fn is_effective_ai() -> bool = is_effective_ai_default;
                /// Returns true when vanilla landing bounce should be suppressed.
                fn is_suppressing_bounce() -> bool = is_suppressing_bounce_default;
                /// Returns true when vanilla block step-on hooks should treat this entity as
                /// careful.
                fn is_stepping_carefully() -> bool = is_stepping_carefully_default;
                /// Updates the vanilla swimming shared flag.
                fn update_swimming() = update_swimming_default;
                /// Returns true if vanilla rules consider this entity to be on a climbable block.
                fn on_climbable() -> bool = on_climbable_default;
                /// Returns the movement vector vanilla exposes for block-contact logic.
                fn known_movement() -> DVec3 = known_movement_default;
                /// Returns the base-tick displacement vanilla exposes as `getKnownSpeed`.
                fn known_speed() -> DVec3 = known_speed_default;
                /// Returns vanilla small and big fall sounds for this entity.
                fn fall_sounds() -> (SoundEventRef, SoundEventRef) = fall_sounds_default;
                /// Rotates this entity to face a fixed position.
                fn look_at(from_anchor: EntityAnchor, target: DVec3) = look_at_default;
                /// Rotates this entity to follow an anchored point on another entity.
                fn look_at_entity(
                    from_anchor: EntityAnchor,
                    target: &dyn Entity,
                    target_anchor: EntityAnchor,
                ) = look_at_entity_default;
                /// Extra spawn-packet data used by vanilla for entity-specific construction.
                fn spawn_data() -> i32 = spawn_data_default;
                /// Returns whether this entity is immune to fire effects and fire damage.
                fn fire_immune() -> bool = fire_immune_default;
                /// Returns vanilla fire immunity cooldown ticks after not being ignited.
                fn fire_immune_ticks() -> i32 = fire_immune_ticks_default;
                /// Returns whether vanilla should play this entity's lava hurt sound.
                fn should_play_lava_hurt_sound() -> bool = should_play_lava_hurt_sound_default;
                /// Maximum ticks this entity's remaining-fire timer can hold; `None` means
                /// unlimited.
                fn remaining_fire_ticks_cap() -> Option<i32> = remaining_fire_ticks_cap_default;
                /// Returns extra ticks added by fire-block ignition before 8-second ignition.
                fn fire_ignite_extra_ticks() -> i32 = fire_ignite_extra_ticks_default;
                /// Returns whether this entity may accumulate frozen ticks.
                fn can_freeze() -> bool = can_freeze_default;
                /// Applies vanilla stuck-in-block movement for the next movement pass.
                fn make_stuck_in_block(
                    state: BlockStateId,
                    speed_multiplier: DVec3,
                ) = make_stuck_in_block_default;
                /// Moves this entity to `pos`, keeping its current rotation.
                fn teleport_to(pos: DVec3) -> Result<(), EntityMoveError> = teleport_to_default;
                /// Returns the block position used for movement-affecting block properties.
                fn block_pos_below_that_affects_movement(
                    ,
                ) -> Option<BlockPos> = block_pos_below_that_affects_movement_default;
                /// Returns vanilla dimensions for a physical pose.
                fn dimensions_for_pose(
                    pose: EntityPose,
                ) -> EntityDimensions = dimensions_for_pose_default;
                /// Returns whether vanilla currently considers this entity crouching.
                fn is_crouching() -> bool = is_crouching_default;
                /// Returns whether vanilla currently considers this entity swimming.
                fn is_swimming() -> bool = is_swimming_default;
                /// Returns vanilla movement side effects emitted by this entity.
                fn movement_emission() -> EntityMovementEmission = movement_emission_default;
                /// Returns whether this entity may modify the world at a position.
                ///
                /// Vanilla `Entity.mayInteract` defaults to true; players apply world
                /// permission checks.
                fn may_interact(world: &World, pos: BlockPos) -> bool = may_interact_default;
                /// Returns this entity's vanilla sound source category.
                fn sound_source() -> SoundSource = sound_source_default;
                /// Returns this entity's vanilla swim sound.
                fn swim_sound() -> SoundEventRef = swim_sound_default;
                /// Plays an entity sound at the entity's exact position.
                fn play_sound(sound: SoundEventRef, volume: f32, pitch: f32) = play_sound_default;
                /// Returns the sound played when this entity enters water at low speed.
                fn swim_splash_sound() -> SoundEventRef = swim_splash_sound_default;
                /// Returns the sound played when this entity enters water at high speed.
                fn swim_high_speed_splash_sound(
                    ,
                ) -> SoundEventRef = swim_high_speed_splash_sound_default;
                /// Plays the vanilla water-entry splash sound and emits `SPLASH`.
                fn do_water_splash_effect() = do_water_splash_effect_default;
                /// Plays the base vanilla step sound for a block.
                fn play_step_sound(
                    pos: BlockPos,
                    block_state: BlockStateId,
                ) = play_step_sound_default;
                /// Returns whether the entity is currently flapping.
                fn is_flapping() -> bool = is_flapping_default;
                /// Runs entity-specific flap side effects.
                fn on_flap() = on_flap_default;
                /// Maximum height this entity can step up during normal movement.
                fn max_up_step() -> f32 = max_up_step_default;
                /// Whether movement should apply player-style sneak edge prevention.
                fn backs_off_from_edge() -> bool = backs_off_from_edge_default;
                /// Gets the default gravity for this entity type.
                ///
                /// Vanilla values: `ItemEntity` = 0.04, `LivingEntity` = 0.08
                fn default_gravity() -> f64 = default_gravity_default;
                /// Applies entity-specific implicit components carried by an item stack.
                ///
                /// Mirrors the overridable part of vanilla's `Entity.applyImplicitComponents`.
                fn apply_implicit_item_components(
                    item_stack: &ItemStack,
                ) = apply_implicit_item_components_default;
                /// Applies damage to this entity.
                fn hurt(world: &World, source: &DamageSource, amount: f32) -> bool = hurt_default;
                /// Runs when this entity causes another entity to die.
                /// The entity provided is the entity who was killed.
                fn killed_entity(
                    world: &World,
                    entity: &Living<'_>,
                    source: &DamageSource,
                ) -> bool = killed_entity_default;
                /// Runs when this entity kills another entity.
                fn award_kill_score(
                    victim: &dyn Entity,
                    killing_blow: &DamageSource,
                ) = award_kill_score_default;
                /// Returns whether this entity is excluded from pressure plates and tripwires.
                ///
                /// Display-like entities and marker entities override this capability.
                fn is_ignoring_block_triggers() -> bool = is_ignoring_block_triggers_default;
                /// Returns how vanilla lets this entity respond to piston movement.
                fn piston_push_reaction() -> PushReaction = piston_push_reaction_default;
                /// Returns whether this entity can accept any passenger.
                fn could_accept_passenger() -> bool = could_accept_passenger_default;
                /// Returns whether this entity handles and consumes an attack before normal damage.
                fn skip_attack_interaction(source: &SharedEntity) -> bool = skip_attack_interaction_default;
                /// Caches a live owner reference after restoring persisted owner-linked
                /// entities. Most entities do not store owner references.
                fn restore_owner_reference(owner: &SharedEntity) = restore_owner_reference_default;
                /// Gets the position to send in this entity's add-entity packet.
                ///
                /// Mirrors vanilla `Entity.getAddEntityPacket()` overloads. Most entities
                /// spawn at their current position; block-attached entities can override
                /// this with the backing block position used by the vanilla packet.
                fn spawn_position() -> DVec3 = spawn_position_default;
                /// Refreshes dimensions for the current physical pose.
                fn refresh_dimensions() = refresh_dimensions_default;
                /// Adds an impulse to this entity's velocity and marks velocity for sync.
                fn push_impulse(impulse: DVec3) = push_impulse_default;
                /// Attempts to set the entity's position through world lifecycle validation.
                ///
                /// For the base method, see [`Self::try_set_position_base`]
                fn try_set_position(pos: DVec3) -> Result<(), EntityMoveError> = try_set_position_default;
                /// Called when a player touches this entity during nearby pickup processing.
                fn player_touch(player: &Arc<Player>) = player_touch_default;
                /// Kills this entity. `world` is vanilla's explicit `ServerLevel`
                /// argument; death game events use the entity's attached world.
                fn kill(world: &World) = kill_default;
                /// Handles vanilla entity right-click interaction.
                fn interact(
                    player: &Player,
                    hand: InteractionHand,
                    location: DVec3,
                ) -> InteractionResult = interact_default;
                /// Returns whether this entity may enter a portal.
                fn can_use_portal(ignore_passenger: bool) -> bool = can_use_portal_default;
                /// Returns the portal cooldown, in ticks, applied after changing dimension.
                fn dimension_changing_delay() -> i32 = dimension_changing_delay_default;
                /// Returns this entity's position relative to the portal, in portal-local space.
                fn relative_portal_position(
                    axis: Axis,
                    portal_area: FoundRectangle,
                ) -> DVec3 = relative_portal_position_default;
                /// Returns this entity's head yaw.
                fn y_head_rot() -> f32 = y_head_rot_default;
                /// Sets this entity's head yaw.
                fn set_y_head_rot(y_head_rot: f32) = set_y_head_rot_default;
                /// Sets this entity's body yaw.
                fn set_y_body_rot(y_body_rot: f32) = set_y_body_rot_default;
                /// Returns whether the entity is currently suffocating inside a solid block.
                fn is_in_wall() -> bool = is_in_wall_default;
                /// Returns the block speed factor applied after movement.
                fn block_speed_factor() -> f32 = block_speed_factor_default;
                /// Returns the horizontal air drag applied after movement.
                fn air_drag() -> f64 = air_drag_default;
                /// Returns the gravity applied this tick.
                fn effective_gravity() -> f64 = effective_gravity_default;
            }
            shared {
                /// Called every game tick when the entity is in a ticked chunk.
                ///
                /// The caller handles post-tick dirty data sync.
                fn tick() = tick_default;
                /// Called every game tick while this entity is riding another entity.
                fn ride_tick() = ride_tick_default;
                /// Applies vanilla fall damage; base entities only propagate it to
                /// passengers and return `false`.
                fn cause_fall_damage(
                    fall_distance: f64,
                    damage_modifier: f32,
                    source: &DamageSource,
                ) -> bool = cause_fall_damage_default;
                /// Accumulates fall distance, applies landing damage from the block
                /// fallen on, and resets it.
                fn check_fall_damage(
                    vertical_movement: f64,
                    on_ground: bool,
                    on_state: BlockStateId,
                    pos: BlockPos,
                    world: &Arc<World>,
                ) = check_fall_damage_default;
                /// Moves this entity by `delta`, resolving collisions.
                fn move_entity(mover_type: MoverType, delta: DVec3) -> Option<MoveResult> = move_entity_default;
            }
        }
    };
}

/// Generates [`EntityProps`], [`EntityHooks`] and [`EntityDefaults`].
macro_rules! entity_hook_items {
    (
        ref { $(
            $(#[$meta:meta])*
            fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? = $default:ident;
        )* }
        shared { $(
            $(#[$smeta:meta])*
            fn $sname:ident($($sarg:ident: $sty:ty),* $(,)?) $(-> $sret:ty)? = $sdefault:ident;
        )* }
    ) => {
        /// Per-type entity hooks, typed for the concrete entity `E`.
        pub struct EntityProps<E: 'static> {
            $(
                $(#[$meta])*
                pub $name: fn(&E $(, $ty)*) $(-> $ret)?,
            )*
            $(
                $(#[$smeta])*
                pub $sname: fn(&E, &SharedEntity $(, $sty)*) $(-> $sret)?,
            )*
        }

        impl<E: Entity> EntityProps<E> {
            /// Every hook at its default.
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    $( $name: |entity $(, $arg)*| entity.$default($($arg),*), )*
                    $( $sname: |_, entity $(, $sarg)*| Arc::clone(entity).$sdefault($($sarg),*), )*
                }
            }
        }

        impl<E: Entity> Default for EntityProps<E> {
            fn default() -> Self {
                Self::new()
            }
        }

        /// [`EntityProps`] reached through `dyn Entity`.
        #[expect(
            clippy::too_many_arguments,
            reason = "hooks take vanilla's arguments plus the entity"
        )]
        pub trait EntityHooks: Send + Sync {
            $(
                $(#[$meta])*
                fn $name(&self, entity: &dyn Entity $(, $arg: $ty)*) $(-> $ret)?;
            )*
            $(
                $(#[$smeta])*
                fn $sname(&self, entity: &SharedEntity $(, $sarg: $sty)*) $(-> $sret)?;
            )*
        }

        impl<E: Entity + DowncastType> EntityHooks for EntityProps<E> {
            $(
                fn $name(&self, entity: &dyn Entity $(, $arg: $ty)*) $(-> $ret)? {
                    match entity.downcast_ref::<E>() {
                        Some(typed) => (self.$name)(typed $(, $arg)*),
                        None => entity.$default($($arg),*),
                    }
                }
            )*
            $(
                fn $sname(&self, entity: &SharedEntity $(, $sarg: $sty)*) $(-> $sret)? {
                    match (**entity).downcast_ref::<E>() {
                        Some(typed) => (self.$sname)(typed, entity $(, $sarg)*),
                        None => Arc::clone(entity).$sdefault($($sarg),*),
                    }
                }
            )*
        }

        /// Every hook at its default, for entity types that override none.
        pub struct EntityDefaults;

        impl EntityHooks for EntityDefaults {
            $(
                fn $name(&self, entity: &dyn Entity $(, $arg: $ty)*) $(-> $ret)? {
                    entity.$default($($arg),*)
                }
            )*
            $(
                fn $sname(&self, entity: &SharedEntity $(, $sarg: $sty)*) $(-> $sret)? {
                    Arc::clone(entity).$sdefault($($sarg),*)
                }
            )*
        }
    };
}

/// Generates the `final` dispatch methods inside [`Entity`].
macro_rules! entity_hook_dispatch {
    (
        ref { $(
            $(#[$meta:meta])*
            fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? = $default:ident;
        )* }
        shared { $(
            $(#[$smeta:meta])*
            fn $sname:ident($($sarg:ident: $sty:ty),* $(,)?) $(-> $sret:ty)? = $sdefault:ident;
        )* }
    ) => {
        $(
            $(#[$meta])*
            final fn $name(&self $(, $arg: $ty)*) $(-> $ret)? {
                self.entity_hooks().$name(self.as_entity_event_source() $(, $arg)*)
            }
        )*
        $(
            $(#[$smeta])*
            final fn $sname(self: Arc<Self> $(, $sarg: $sty)*) $(-> $sret)? {
                let entity = self.into_shared_entity();
                entity.entity_hooks().$sname(&entity $(, $sarg)*)
            }
        )*
    };
}

pub(crate) use {entity_hook_dispatch, entity_hook_list};

entity_hook_list!(entity_hook_items);
