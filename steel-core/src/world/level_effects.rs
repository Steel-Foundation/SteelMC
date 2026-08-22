use super::{
    Arc, BLOCK_BEHAVIORS, BlockLootContext, BlockPos, BlockStateExt, BlockStateId, CLevelEvent,
    CLevelParticles, CSound, ChunkPos, ConnectionProtocol, DVec3, EncodedPacket, Entity,
    GLOBAL_SOUND_EVENTS, GameEventContext, ItemStack, LevelReader, LootContext, NetworkConnection,
    ParticleData, Player, REGISTRY, RegistryExt, SectionPos, SoundEventRef, SoundSource,
    UpdateFlags, World, WorldEntityManager, entity_loot_ref, fluid_state_to_block, level_events,
    vanilla_blocks, vanilla_game_events,
};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
pub(super) fn sound_is_within_range(
    sound: SoundEventRef,
    volume: f32,
    distance_squared: f64,
) -> bool {
    let range = f64::from(sound.range(volume));
    distance_squared < range * range
}

impl World {
    /// Broadcasts a level event to nearby players within 64 blocks.
    ///
    /// Level events trigger sounds, particles, and animations on the client.
    /// See `steel_registry::level_events` for available event type constants.
    ///
    /// # Arguments
    /// * `event_type` - The event type ID from `steel_registry::level_events`
    /// * `pos` - The position where the event occurs
    /// * `data` - Event-specific data (e.g., block state ID for block destruction)
    /// * `exclude` - Optional entity ID to exclude from receiving the event
    pub fn level_event(&self, event_type: i32, pos: BlockPos, data: i32, exclude: Option<i32>) {
        let packet = CLevelEvent::new(event_type, pos, data, false);
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode level event packet");
            return;
        };

        self.players.iter_players(|_, player| {
            if exclude != Some(player.id())
                && Self::recipient_within_64_blocks(player.position(), pos)
            {
                player.connection.send_encoded(encoded.clone());
            }
            true
        });
    }

    pub(super) fn recipient_within_64_blocks(player_pos: DVec3, event_pos: BlockPos) -> bool {
        const MAX_DISTANCE_SQ: f64 = 64.0 * 64.0;

        let dx = f64::from(event_pos.x()) - player_pos.x;
        let dy = f64::from(event_pos.y()) - player_pos.y;
        let dz = f64::from(event_pos.z()) - player_pos.z;
        dx * dx + dy * dy + dz * dz < MAX_DISTANCE_SQ
    }

    /// Sends a particle distribution to every player within Vanilla's normal
    /// 32-block particle radius.
    pub fn send_particles(
        &self,
        particle: ParticleData,
        position: DVec3,
        count: i32,
        spread: DVec3,
        speed: f64,
    ) -> i32 {
        self.send_particles_with_options(particle, false, false, position, count, spread, speed)
    }

    /// Sends a particle distribution with the packet visibility flags selected
    /// explicitly. `override_limiter` also expands the server recipient radius
    /// from 32 to 512 blocks, matching `ServerLevel.sendParticles`.
    #[expect(
        clippy::too_many_arguments,
        reason = "keeps Vanilla's two particle visibility flags explicit"
    )]
    pub fn send_particles_with_options(
        &self,
        particle: ParticleData,
        override_limiter: bool,
        always_show: bool,
        position: DVec3,
        count: i32,
        spread: DVec3,
        speed: f64,
    ) -> i32 {
        let packet = CLevelParticles {
            override_limiter,
            always_show,
            x: position.x,
            y: position.y,
            z: position.z,
            x_dist: spread.x as f32,
            y_dist: spread.y as f32,
            z_dist: spread.z as f32,
            max_speed: speed as f32,
            count,
            particle,
        };
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode level particles packet");
            return 0;
        };
        let mut sent = 0;
        self.players.iter_players(|_, player| {
            if Self::particle_recipient_in_range(
                player.block_position(),
                position,
                override_limiter,
            ) {
                player.connection.send_encoded(encoded.clone());
                sent += 1;
            }
            true
        });
        sent
    }

    /// Sends a particle distribution to one player if they are in this world
    /// and within Vanilla's particle recipient radius.
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors Vanilla ServerLevel.sendParticles"
    )]
    pub fn send_particles_to(
        self: &Arc<Self>,
        player: &Player,
        particle: ParticleData,
        override_limiter: bool,
        always_show: bool,
        position: DVec3,
        count: i32,
        spread: DVec3,
        speed: f64,
    ) -> bool {
        if !Arc::ptr_eq(self, &player.get_world())
            || !Self::particle_recipient_in_range(
                player.block_position(),
                position,
                override_limiter,
            )
        {
            return false;
        }

        let packet = CLevelParticles {
            override_limiter,
            always_show,
            x: position.x,
            y: position.y,
            z: position.z,
            x_dist: spread.x as f32,
            y_dist: spread.y as f32,
            z_dist: spread.z as f32,
            max_speed: speed as f32,
            count,
            particle,
        };
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode level particles packet");
            return false;
        };
        player.connection.send_encoded(encoded);
        true
    }

    pub(super) fn particle_recipient_in_range(
        player_block_pos: BlockPos,
        particle_pos: DVec3,
        override_limiter: bool,
    ) -> bool {
        let (x, y, z) = player_block_pos.get_center();
        let radius = if override_limiter { 512.0 } else { 32.0 };
        DVec3::new(x, y, z).distance_squared(particle_pos) < radius * radius
    }

    /// Broadcasts a global level event to all players in the world.
    ///
    /// When `global_sound_events` is disabled, vanilla falls back to a normal
    /// nearby level event with the packet's global flag unset.
    ///
    /// # Arguments
    /// * `event_type` - The event type ID from `steel_registry::level_events`
    /// * `pos` - The position where the event occurs
    /// * `data` - Event-specific data
    pub fn global_level_event(&self, event_type: i32, pos: BlockPos, data: i32) {
        if !self.get_game_rule(&GLOBAL_SOUND_EVENTS) {
            self.level_event(event_type, pos, data, None);
            return;
        }

        let packet = CLevelEvent::new(event_type, pos, data, true);
        self.players.iter_players(|_, player| {
            player.send_packet(packet.clone());
            true
        });
    }

    /// Broadcasts block destruction particles and sound for a destroyed block.
    ///
    /// This is a convenience method that sends the `PARTICLES_DESTROY_BLOCK` level event.
    ///
    /// # Arguments
    /// * `pos` - The position of the destroyed block
    /// * `block_state_id` - The block state ID of the destroyed block
    /// * `exclude` - Optional entity ID to exclude from receiving the event
    pub fn destroy_block_effect(&self, pos: BlockPos, block_state_id: u32, exclude: Option<i32>) {
        self.level_event(
            level_events::PARTICLES_DESTROY_BLOCK,
            pos,
            block_state_id as i32,
            exclude,
        );
    }

    /// Destroys a block at the given position, optionally dropping its loot.
    ///
    /// Sends destruction particles (skipping fire blocks), optionally drops
    /// resources via loot table, then replaces with air.
    ///
    /// Defaults to recursion limit of 512
    pub fn destroy_block(self: &Arc<Self>, pos: BlockPos, drop_items: bool) -> bool {
        self.destroy_block_with_limit(pos, drop_items, 512)
    }

    /// Replaces a block with its fluid state's legacy block.
    ///
    /// Mirrors vanilla `Level.removeBlock`, including the piston-move update flag.
    pub fn remove_block(self: &Arc<Self>, pos: BlockPos, moved_by_piston: bool) -> bool {
        let state = self.get_block_state(pos);
        let replacement = fluid_state_to_block(state.get_fluid_state());
        let mut flags = UpdateFlags::UPDATE_ALL;
        if moved_by_piston {
            flags |= UpdateFlags::UPDATE_MOVE_BY_PISTON;
        }
        self.set_block(pos, replacement, flags)
    }

    /// Destroys a block with an entity source for game-event context.
    pub fn destroy_block_by_entity(
        self: &Arc<Self>,
        pos: BlockPos,
        drop_items: bool,
        entity: &dyn Entity,
    ) -> bool {
        self.destroy_block_with_limit_and_entity(pos, drop_items, 512, Some(entity))
    }

    /// Destroys a block at the given position, optionally dropping its loot.
    ///
    /// Sends destruction particles (skipping fire blocks), optionally drops
    /// resources via loot table, then replaces with air.
    pub fn destroy_block_with_limit(
        self: &Arc<Self>,
        pos: BlockPos,
        drop_items: bool,
        recursion_left: i32,
    ) -> bool {
        self.destroy_block_with_limit_and_entity(pos, drop_items, recursion_left, None)
    }

    pub(super) fn destroy_block_with_limit_and_entity(
        self: &Arc<Self>,
        pos: BlockPos,
        drop_items: bool,
        recursion_left: i32,
        entity: Option<&dyn Entity>,
    ) -> bool {
        let state = self.get_block_state(pos);
        if state.is_air() {
            return false;
        }

        let block = state.get_block();
        let is_fire = block == &vanilla_blocks::FIRE || block == &vanilla_blocks::SOUL_FIRE;
        if !is_fire {
            self.destroy_block_effect(pos, u32::from(state.0), None);
        }

        if drop_items {
            self.drop_resources_with_entity(state, pos, entity);
            // TODO: This only covers the `drop_items` path. In vanilla, container
            // content dropping runs unconditionally on any block-entity removal
            // (BlockEntity.preRemoveSideEffects via LevelChunk.setBlockState) —
            // independent of drop_items — so explosions, pistons, etc. still need
            // a similar hook once Steel's block-update pipeline has one.
            if let Some(block_entity) = self.get_block_entity(pos)
                && let Some(container_ref) = ContainerRef::from_block_entity(block_entity)
            {
                let mut guard = ContainerLockGuard::lock_all(&[&container_ref]);
                if let Some(container) = guard.get_mut(container_ref.container_id()) {
                    for slot in 0..container.get_container_size() {
                        let item = container.remove_item_no_update(slot);
                        if !item.is_empty() {
                            self.pop_resource(pos, item);
                        }
                    }
                }
            }
        }

        // Vanilla parity: fluidState.createLegacyBlock() — breaking a waterlogged
        // block leaves water behind instead of air.
        let replacement = fluid_state_to_block(state.get_fluid_state());
        let destroyed =
            self.set_block_with_limit(pos, replacement, UpdateFlags::UPDATE_ALL, recursion_left);
        if destroyed {
            self.game_event(
                &vanilla_game_events::BLOCK_DESTROY,
                pos,
                &GameEventContext::new(entity, Some(state)),
            );
        }
        destroyed
    }

    /// Drops the loot for a block using its loot table.
    ///
    /// This is the no-tool/no-entity overload. Player block breaking uses
    /// `block_breaking::drop_block_loot` which includes tool context for
    /// fortune/silk touch.
    // TODO: block entity and entity drops
    pub fn drop_resources(self: &Arc<Self>, state: BlockStateId, pos: BlockPos) {
        self.drop_resources_with_entity(state, pos, None);
    }

    pub(super) fn drop_resources_with_entity(
        self: &Arc<Self>,
        state: BlockStateId,
        pos: BlockPos,
        entity: Option<&dyn Entity>,
    ) {
        let context = BlockLootContext::new(self, pos).with_entity(entity);
        for item in context.get_drops(state) {
            if !item.is_empty() {
                self.pop_resource(pos, item);
            }
        }
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .spawn_after_break(state, self, pos, &ItemStack::empty(), true);
    }

    pub(crate) fn block_drops(
        state: BlockStateId,
        context: &BlockLootContext<'_>,
    ) -> Vec<ItemStack> {
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        behavior
            .get_drops(state, context)
            .unwrap_or_else(|| Self::default_block_drops(state, context))
    }

    pub(super) fn default_block_drops(
        state: BlockStateId,
        context: &BlockLootContext<'_>,
    ) -> Vec<ItemStack> {
        let block = state.get_block();
        let loot_key = steel_utils::Identifier::vanilla(format!("blocks/{}", block.key.path));

        let Some(loot_table) = REGISTRY.loot_tables.by_key(&loot_key) else {
            return Vec::new();
        };

        let mut rng = rand::rng();
        let mut ctx = LootContext::new(&mut rng)
            .with_luck(context.luck())
            .with_block_state(state)
            .with_origin(
                f64::from(context.pos().x()),
                f64::from(context.pos().y()),
                f64::from(context.pos().z()),
            );
        if let Some(tool) = context.tool() {
            ctx = ctx.with_tool(tool);
        }
        if let Some(entity) = context.entity() {
            ctx = ctx.with_this_entity(entity_loot_ref(entity));
        }

        loot_table.get_random_items(&mut ctx)
    }

    /// Plays a sound at a specific position, broadcasting to nearby players.
    ///
    /// The sound is sent to players within its vanilla range, except for the
    /// excluded player (if any). The excluded player is typically the one who
    /// triggered the sound, as they hear it client-side.
    ///
    /// # Arguments
    /// * `sound` - The sound event to play
    /// * `source` - The sound source category
    /// * `pos` - The block position (sound plays at center of block)
    /// * `volume` - Volume multiplier (1.0 = normal)
    /// * `pitch` - Pitch multiplier (1.0 = normal)
    /// * `exclude` - Optional entity ID to exclude from receiving the sound
    pub fn play_sound(
        &self,
        sound: SoundEventRef,
        source: SoundSource,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        self.play_sound_at(
            sound,
            source,
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            volume,
            pitch,
            exclude,
        );
    }

    /// Plays a sound at an exact world position, broadcasting to nearby players.
    pub fn play_sound_at(
        &self,
        sound: SoundEventRef,
        source: SoundSource,
        pos: DVec3,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x.floor() as i32),
            SectionPos::block_to_section_coord(pos.z.floor() as i32),
        );

        // Generate a random seed for sound variations
        let seed = rand::random::<i64>();
        let packet = CSound::new(sound, source, pos, volume, pitch, seed);
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.compression, ConnectionProtocol::Play)
        else {
            log::warn!("Failed to encode sound packet");
            return;
        };

        // Get players tracking this chunk, then apply vanilla's strict range check.
        for entity_id in self.player_area_map.get_tracking_players(chunk) {
            // Skip excluded player (they hear the sound client-side)
            if exclude == Some(entity_id) {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                let player_pos = player.position();
                let dx = player_pos.x - pos.x;
                let dy = player_pos.y - pos.y;
                let dz = player_pos.z - pos.z;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if sound_is_within_range(sound, volume, dist_sq) {
                    player.connection.send_encoded(encoded.clone());
                }
            }
        }
    }

    /// Plays a block sound at a specific position.
    ///
    /// Convenience method that uses the BLOCKS sound source and applies
    /// the sound type's volume and pitch modifiers.
    ///
    /// # Arguments
    /// * `sound` - The sound event to play
    /// * `pos` - The block position
    /// * `volume` - Base volume (typically from `SoundType`)
    /// * `pitch` - Base pitch (typically from `SoundType`)
    /// * `exclude` - Optional entity ID to exclude from receiving the sound
    pub fn play_block_sound(
        &self,
        sound: SoundEventRef,
        pos: BlockPos,
        volume: f32,
        pitch: f32,
        exclude: Option<i32>,
    ) {
        self.play_sound(sound, SoundSource::Blocks, pos, volume, pitch, exclude);
    }

    /// Returns the runtime entity manager.
    #[must_use]
    pub(crate) const fn entity_manager(&self) -> &WorldEntityManager {
        &self.entity_manager
    }
}
