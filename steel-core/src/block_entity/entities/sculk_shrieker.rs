//! Sculk Shrieker block entity - tracks warnings and spawns Wardens.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::game_events::GameEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_game_events;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, Downcast, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::Entity;
use crate::player::warden_spawn_tracker::WardenSpawnResult;
use crate::world::game_event::{GameEventContext, GameEventDeliveryMode, GameEventListener};
use crate::world::World;

const COOLDOWN_TICKS: i32 = 10 * 20; // 10 seconds
const WARNING_DISTANCE: f64 = 16.0;

struct ShriekerState {
    cooldown: i32,
    can_summon: bool,
}

/// Sculk Shrieker block entity - tracks warnings and spawns Wardens.
pub struct SculkShriekerBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<ShriekerState>,
    listener: Arc<SculkShriekerListener>,
}

unsafe impl DowncastType for SculkShriekerBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/sculk_shrieker");
}

impl SculkShriekerBlockEntity {
    /// Creates a new sculk shrieker block entity.
    #[must_use]
    pub fn new(pos: BlockPos, state: BlockStateId, level: Weak<World>) -> Self {
        let listener = Arc::new(SculkShriekerListener { pos });
        Self {
            base: BlockEntityBase::new(
                &vanilla_block_entity_types::SCULK_SHRIEKER,
                level,
                pos,
                state,
            ),
            state: SyncMutex::new(ShriekerState {
                cooldown: 0,
                can_summon: false, // Player-placed shriekers can't summon
            }),
            listener,
        }
    }

    /// Ticks the shrieker to handle cooldown.
    pub fn tick(&self, _world: &Arc<World>) {
        let mut state = self.state.lock();
        if state.cooldown > 0 {
            state.cooldown -= 1;
        }
    }

    /// Called from block behavior when shrieking state ends
    pub fn try_respond(&self, _world: &Arc<World>) {
        // Shrieking animation complete - no additional action needed
        // Warning was already issued when vibration was received
    }

    /// Called from block behavior step_on
    pub fn try_shriek_from_entity(&self, world: &Arc<World>, entity: &dyn Entity) {
        // Step-on doesn't trigger shriek in vanilla - only vibrations do
        let _ = (world, entity);
    }

    fn try_shriek(&self, world: &Arc<World>, player_id: Option<i32>) {
        let mut state = self.state.lock();
        if state.cooldown > 0 {
            return;
        }

        state.cooldown = COOLDOWN_TICKS;
        let can_summon = state.can_summon;
        drop(state);

        // Play shriek sound and particles
        self.play_shriek_effects(world);

        // Apply Darkness and update warnings via persistent player tracker
        if let Some(pid) = player_id {
            self.apply_darkness_to_player(world, pid);
            self.increment_warning(world, pid, can_summon);
        } else {
            self.apply_darkness_to_nearby_players(world);
        }

        self.set_changed();
    }

    fn play_shriek_effects(&self, world: &Arc<World>) {
        let pos = self.base.pos();

        // Play shriek sound
        world.play_sound(
            &steel_registry::sound_events::BLOCK_SCULK_SHRIEKER_SHRIEK,
            SoundSource::Blocks,
            pos,
            3.0,
            1.0,
            None,
        );

        // TODO: Emit shriek game event
        let context = GameEventContext::default();
        world.game_event(&vanilla_game_events::SHRIEK, pos, &context);

        // TODO: Spawn shriek particles
    }

    fn apply_darkness_to_nearby_players(&self, world: &Arc<World>) {
        let (px, py, pz) = self.base.pos().get_center();
        let pos = DVec3::new(px, py, pz);
        world.players.iter_players(|_, player| {
            if player.position().distance_squared(pos) < WARNING_DISTANCE * WARNING_DISTANCE {
                self.apply_darkness_effect(player);
            }
            true
        });
    }

    fn apply_darkness_to_player(&self, world: &Arc<World>, player_id: i32) {
        if let Some(player) = world.players.get_by_entity_id(player_id) {
            self.apply_darkness_effect(&player);
        }
    }

    fn apply_darkness_effect(&self, player: &crate::player::Player) {
        // Apply Darkness mob effect - 12 seconds (240 ticks) at amplifier 0
        use crate::entity::LivingEntity;
        use steel_registry::vanilla_mob_effects;

        const DARKNESS_DURATION_TICKS: i32 = 12 * 20; // 12 seconds

        let effect = crate::entity::ActiveMobEffect::with_duration(
            vanilla_mob_effects::DARKNESS,
            DARKNESS_DURATION_TICKS,
            0, // amplifier 0
        );

        player.living_base().add_mob_effect(effect);
        log::debug!("Applied Darkness effect to player {}", player.gameprofile.name);
    }

    fn increment_warning(&self, world: &Arc<World>, player_id: i32, can_summon: bool) {
        let Some(player) = world.players.get_by_entity_id(player_id) else {
            return;
        };

        let game_time = world.game_time();
        let mut tracker = player.warden_spawn_tracker().lock();

        let result = tracker.try_warn(game_time, can_summon);
        drop(tracker);

        match result {
            WardenSpawnResult::OnCooldown => {
                log::debug!("Player {} on Warden spawn cooldown", player.gameprofile.name);
            }
            WardenSpawnResult::Warning { level } => {
                log::debug!(
                    "Player {} warning level: {}/4 at {:?}",
                    player.gameprofile.name,
                    level,
                    self.base.pos()
                );
            }
            WardenSpawnResult::SpawnWarden => {
                log::info!("Spawning Warden for player {}", player.gameprofile.name);
                self.try_spawn_warden(world, player_id);
            }
        }
    }

    fn try_spawn_warden(&self, world: &Arc<World>, _player_id: i32) {
        let pos = self.base.pos();

        // Find suitable spawn location near shrieker
        if let Some(spawn_pos) = self.find_warden_spawn_pos(world, pos) {
            log::info!("Spawning Warden at {:?}", spawn_pos);

            // Spawn Warden entity
            let (wx, wy, wz) = spawn_pos.get_center();
            let warden_pos = DVec3::new(wx, wy, wz);

            let entity_id = crate::entity::next_entity_id();
            let Some(warden) = crate::entity::ENTITIES.create(
                &vanilla_entities::WARDEN,
                entity_id,
                warden_pos,
                std::sync::Arc::downgrade(world),
            ) else {
                log::warn!("Failed to create Warden entity");
                return;
            };

            if world.try_add_entity(warden).is_ok() {
                log::info!("Warden spawned successfully");

                // Play spawn effects
                world.play_sound(
                    &steel_registry::sound_events::ENTITY_WARDEN_EMERGE,
                    SoundSource::Hostile,
                    spawn_pos,
                    3.0,
                    1.0,
                    None,
                );
            } else {
                log::warn!("Failed to add Warden to world at {:?}", spawn_pos);
            }
        } else {
            log::warn!("Could not find suitable Warden spawn location near {:?}", pos);
        }
    }

    fn find_warden_spawn_pos(&self, world: &Arc<World>, center: BlockPos) -> Option<BlockPos> {
        // Search for valid spawn location within 5 blocks horizontally, 3 vertically
        // Check center first for common case
        if self.is_valid_spawn_location(world, center) {
            return Some(center);
        }

        // Then check nearby positions in expanding rings
        for radius in 1i32..=5i32 {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    // Only check perimeter of current radius
                    if dx.abs() != radius && dz.abs() != radius {
                        continue;
                    }

                    for dy in -3i32..=3i32 {
                        let check_pos = BlockPos::new(
                            center.x() + dx,
                            center.y() + dy,
                            center.z() + dz,
                        );

                        if self.is_valid_spawn_location(world, check_pos) {
                            return Some(check_pos);
                        }
                    }
                }
            }
        }
        None
    }

    fn is_valid_spawn_location(&self, world: &Arc<World>, pos: BlockPos) -> bool {
        // Check ground is solid
        let ground = world.get_block_state(BlockPos::new(pos.x(), pos.y() - 1, pos.z()));
        if ground.is_air() {
            return false;
        }

        // Check 3 blocks of air space above (Warden height ~2.9)
        for i in 0..3 {
            let check = world.get_block_state(BlockPos::new(pos.x(), pos.y() + i, pos.z()));
            if !check.is_air() {
                return false;
            }
        }

        true
    }
}

impl BlockEntity for SculkShriekerBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();

        let mut state = self.state.lock();

        if let Some(cooldown) = nbt_view.int("cooldown") {
            state.cooldown = cooldown;
        }
        if let Some(can_summon) = nbt_view.byte("can_summon") {
            state.can_summon = can_summon != 0;
        }

        // Note: Per-player warnings are now stored in player data, not BlockEntity
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert("cooldown", state.cooldown);
        nbt.insert("can_summon", if state.can_summon { 1i8 } else { 0i8 });

        // Note: Per-player warnings are now stored in player data, not BlockEntity
    }

    fn tick(&self, world: &Arc<World>) {
        self.tick(world);
    }

    fn game_event_listener(&self) -> Option<crate::world::game_event::SharedGameEventListener> {
        Some(Arc::clone(&self.listener) as _)
    }
}

struct SculkShriekerListener {
    pos: BlockPos,
}

impl GameEventListener for SculkShriekerListener {
    fn listener_pos(&self) -> Option<DVec3> {
        let (x, y, z) = self.pos.get_center();
        Some(DVec3::new(x, y, z))
    }

    fn listener_radius(&self) -> i32 {
        8
    }

    fn delivery_mode(&self) -> GameEventDeliveryMode {
        GameEventDeliveryMode::ByDistance
    }

    fn handle_game_event(
        &self,
        world: &Arc<World>,
        _event: GameEventRef,
        context: &GameEventContext<'_>,
        _source_pos: DVec3,
    ) -> bool {
        // Get player who caused the event
        let player_id = context.source_entity().and_then(|e| {
            if e.entity_type().key.path == "player" {
                Some(e.id())
            } else {
                None
            }
        });

        // Trigger shriek
        if let Some(block_entity) = world.get_block_entity(self.pos) {
            if let Some(shrieker) = block_entity.downcast_ref::<SculkShriekerBlockEntity>() {
                shrieker.try_shriek(world, player_id);
                return true;
            }
        }
        false
    }
}
