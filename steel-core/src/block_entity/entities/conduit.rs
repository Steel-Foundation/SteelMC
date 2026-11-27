//! Conduit block entity implementation.
//!
//! Vanilla's client-only `effectBlocks` list, `tickCount` and `activeRotation` are not kept;
//! the server only needs the frame size from each scan.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_block_entity_types, vanilla_block_tags,
    vanilla_damage_types, vanilla_entities, vanilla_mob_effects,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{
    BlockLocalAabb, BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey,
};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity as _, EntityReference, LivingEntity as _, MobEffectInstance, SharedEntity,
};
use crate::fluid::{FluidStateExt as _, get_fluid_state};
use crate::player::Player;
use crate::world::World;

const BLOCK_REFRESH_RATE: i64 = 40;
const AMBIENT_SOUND_INTERVAL: i64 = 80;
const EFFECT_DURATION: i32 = 13 * 20;
const MIN_ACTIVE_SIZE: usize = 16;
const MIN_KILL_SIZE: usize = 42;
const KILL_RANGE: f64 = 8.0;
const ATTACK_DAMAGE: f32 = 4.0;

/// The frame positions around the conduit, in vanilla's x, y, z scan order: outside the
/// 3x3x3 water cage, on one of the three axis-aligned rings of radius 2.
const FRAME_OFFSETS: [(i32, i32, i32); MIN_KILL_SIZE] = {
    let mut offsets = [(0, 0, 0); MIN_KILL_SIZE];
    let mut count = 0;
    let mut ox: i32 = -2;
    while ox <= 2 {
        let mut oy: i32 = -2;
        while oy <= 2 {
            let mut oz: i32 = -2;
            while oz <= 2 {
                let (ax, ay, az) = (ox.abs(), oy.abs(), oz.abs());
                let outside_cage = ax > 1 || ay > 1 || az > 1;
                let on_ring = ox == 0 && (ay == 2 || az == 2)
                    || oy == 0 && (ax == 2 || az == 2)
                    || oz == 0 && (ax == 2 || ay == 2);
                if outside_cage && on_ring {
                    offsets[count] = (ox, oy, oz);
                    count += 1;
                }
                oz += 1;
            }
            oy += 1;
        }
        ox += 1;
    }
    assert!(
        count == MIN_KILL_SIZE,
        "a full conduit frame is the hunting size"
    );
    offsets
};

struct ConduitState {
    is_active: bool,
    is_hunting: bool,
    destroy_target: Option<EntityReference>,
    next_ambient_sound_activation: i64,
}

/// Conduit block entity.
pub struct ConduitBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<ConduitState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ConduitBlockEntity`.
unsafe impl DowncastType for ConduitBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/conduit");
}

impl ConduitBlockEntity {
    /// Creates a new conduit block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::CONDUIT, level, pos, state),
            state: SyncMutex::new(ConduitState {
                is_active: false,
                is_hunting: false,
                destroy_target: None,
                next_ambient_sound_activation: 0,
            }),
        }
    }

    /// Returns whether the last shape check found a valid water cage and frame.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state.lock().is_active
    }

    /// Returns whether the last shape check found a frame large enough to attack enemies.
    #[must_use]
    pub fn is_hunting(&self) -> bool {
        self.state.lock().is_hunting
    }

    /// Returns the number of frame blocks, or `0` when the surrounding 3x3x3 is not all water.
    fn update_shape(world: &Arc<World>, pos: BlockPos) -> usize {
        for ox in -1..=1 {
            for oy in -1..=1 {
                for oz in -1..=1 {
                    if !get_fluid_state(world, pos.offset(ox, oy, oz)).is_water() {
                        return 0;
                    }
                }
            }
        }

        FRAME_OFFSETS
            .iter()
            .filter(|&&(ox, oy, oz)| {
                let state = world.get_block_state(pos.offset(ox, oy, oz));
                REGISTRY.blocks.is_in_tag(
                    state.get_block(),
                    &vanilla_block_tags::BlockTag::CONDUIT_EFFECT_BLOCK,
                )
            })
            .count()
    }

    fn apply_effects(world: &Arc<World>, pos: BlockPos, active_size: usize) {
        let effect_range = f64::from((active_size / 7 * 16) as i32);
        let aabb = BlockLocalAabb::FULL_BLOCK
            .at_block(pos)
            .inflate(effect_range)
            .expand_towards(DVec3::new(0.0, f64::from(world.get_height()), 0.0));

        for entity in world.get_entities_in_aabb_matching(&aabb, |entity| {
            entity.entity_type() == &vanilla_entities::PLAYER && !entity.is_spectator()
        }) {
            let Some(player) = entity.downcast_ref::<Player>() else {
                continue;
            };
            if !pos.closer_than(player.block_position(), effect_range)
                || !player.is_in_water_or_rain()
            {
                continue;
            }
            player.add_mob_effect(
                MobEffectInstance::with_duration(
                    vanilla_mob_effects::CONDUIT_POWER,
                    EFFECT_DURATION,
                    0,
                )
                .with_ambient(true)
                .with_visible(true),
            );
        }
    }

    fn update_and_attack_target(&self, world: &Arc<World>, pos: BlockPos, is_hunting: bool) {
        let current_target = self.state.lock().destroy_target.clone();
        let new_target =
            Self::update_destroy_target(current_target.as_ref(), world, pos, is_hunting);

        if let Some(target) = new_target
            .as_ref()
            .and_then(|t| Self::living_entity(t, world))
        {
            world.play_sound_at(
                &sound_events::BLOCK_CONDUIT_ATTACK_TARGET,
                SoundSource::Blocks,
                target.position(),
                1.0,
                1.0,
                None,
            );
            if let Some(living) = target.as_living_entity() {
                living.hurt_server(
                    world,
                    &DamageSource::environment(&vanilla_damage_types::MAGIC),
                    ATTACK_DAMAGE,
                );
            }
        }

        let changed = {
            let mut state = self.state.lock();
            let changed = state.destroy_target.as_ref().map(EntityReference::uuid)
                != new_target.as_ref().map(EntityReference::uuid);
            if changed {
                state.destroy_target = new_target;
            }
            changed
        };
        if changed {
            world.send_block_updated(pos);
        }
    }

    fn update_destroy_target(
        target: Option<&EntityReference>,
        world: &Arc<World>,
        pos: BlockPos,
        is_hunting: bool,
    ) -> Option<EntityReference> {
        if !is_hunting {
            return None;
        }

        let Some(target) = target else {
            return Self::select_new_target(world, pos);
        };

        let entity = Self::living_entity(target, world)?;
        (entity.is_alive() && pos.closer_than(entity.block_position(), KILL_RANGE))
            .then(|| target.clone())
    }

    fn select_new_target(world: &Arc<World>, pos: BlockPos) -> Option<EntityReference> {
        let aabb = BlockLocalAabb::FULL_BLOCK.at_block(pos).inflate(KILL_RANGE);
        let candidates = world.get_entities_in_aabb_matching(&aabb, |entity| {
            entity.as_living_entity().is_some() && entity.is_enemy() && entity.is_in_water_or_rain()
        });
        if candidates.is_empty() {
            return None;
        }
        let chosen = &candidates[rand::random_range(0..candidates.len())];
        Some(EntityReference::from_entity(chosen))
    }

    fn living_entity(reference: &EntityReference, world: &World) -> Option<SharedEntity> {
        reference
            .get_entity(world)
            .filter(|entity| entity.as_living_entity().is_some())
    }
}

impl BlockEntity for ConduitBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        self.state.lock().destroy_target = EntityReference::read(&nbt_view, "Target");
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        if let Some(target) = &self.state.lock().destroy_target {
            target.store(nbt, "Target");
        }
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        Some(self.save_custom_only())
    }

    fn tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let game_time = world.game_time();

        if game_time % BLOCK_REFRESH_RATE == 0 {
            let effect_blocks = Self::update_shape(world, pos);
            let active = effect_blocks >= MIN_ACTIVE_SIZE;
            let hunting = effect_blocks >= MIN_KILL_SIZE;

            let was_active = {
                let mut state = self.state.lock();
                let was_active = state.is_active;
                state.is_active = active;
                state.is_hunting = hunting;
                was_active
            };
            if active != was_active {
                let sound = if active {
                    &sound_events::BLOCK_CONDUIT_ACTIVATE
                } else {
                    &sound_events::BLOCK_CONDUIT_DEACTIVATE
                };
                world.play_block_sound(sound, pos, 1.0, 1.0, None);
            }

            if active {
                Self::apply_effects(world, pos, effect_blocks);
                self.update_and_attack_target(world, pos, hunting);
            }
        }

        let play_short_ambient = {
            let mut state = self.state.lock();
            if !state.is_active {
                return;
            }
            let play = game_time > state.next_ambient_sound_activation;
            if play {
                state.next_ambient_sound_activation = game_time + 60 + rand::random_range(0..40);
            }
            play
        };

        if game_time % AMBIENT_SOUND_INTERVAL == 0 {
            world.play_block_sound(&sound_events::BLOCK_CONDUIT_AMBIENT, pos, 1.0, 1.0, None);
        }
        if play_short_ambient {
            world.play_block_sound(
                &sound_events::BLOCK_CONDUIT_AMBIENT_SHORT,
                pos,
                1.0,
                1.0,
                None,
            );
        }
    }
}
