//! Beacon block entity implementation.
//!
//! Beacons track the pyramid level and configured effects. The beam is re-scanned
//! incrementally, [`BLOCKS_CHECK_PER_TICK`] blocks per tick, and every 80 game ticks the
//! supporting pyramid is re-checked and the configured status effects are applied to nearby
//! players while the beam is unobstructed.

use std::{
    mem,
    str::FromStr as _,
    sync::{Arc, Weak},
};

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::mob_effect::MobEffectRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt, sound_events, vanilla_block_entity_types,
    vanilla_block_tags, vanilla_blocks, vanilla_entities, vanilla_mob_effects,
};
use steel_utils::color::ArgbColor;
use steel_utils::locks::SyncMutex;
use steel_utils::{
    BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey, Identifier, WorldAabb,
};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::chunk::heightmap::HeightmapType;
use crate::chunk::light::MAX_LIGHT_LEVEL;
use crate::entity::{LivingEntity as _, MobEffectInstance};
use crate::player::Player;
use crate::world::World;

/// Maximum beacon pyramid level.
const MAX_LEVELS: i32 = 4;

const BEACON_TICK_INTERVAL: i64 = 80;

/// Blocks of beam column scanned per tick, mirroring vanilla `BLOCKS_CHECK_PER_TICK`.
const BLOCKS_CHECK_PER_TICK: i32 = 10;

const BASE_EFFECT_RANGE: f64 = 10.0;
const EFFECT_RANGE_PER_LEVEL: f64 = 10.0;

/// The four valid beacon effects, indexed by pyramid level tier.
pub(crate) const BEACON_EFFECTS: [&[MobEffectRef]; 4] = [
    &[vanilla_mob_effects::SPEED, vanilla_mob_effects::HASTE],
    &[
        vanilla_mob_effects::RESISTANCE,
        vanilla_mob_effects::JUMP_BOOST,
    ],
    &[vanilla_mob_effects::STRENGTH],
    &[vanilla_mob_effects::REGENERATION],
];

/// A run of beam column sharing one tint.
///
/// Vanilla parity: `BeaconBeamOwner.Section`. Only emptiness is read server-side today; the tint
/// and height are tracked so the scan splits sections exactly as Vanilla does.
// TODO: Expose these through a `getBeamSections` equivalent when something server-side needs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeamSection {
    color: ArgbColor,
    height: i32,
}

impl BeamSection {
    const fn new(color: ArgbColor) -> Self {
        Self { color, height: 1 }
    }

    const fn increase_height(&mut self) {
        self.height += 1;
    }
}

/// Mutable beacon state shared with the menu's data slots.
pub struct BeaconState {
    pub(crate) levels: i32,
    pub(crate) primary_power: Option<MobEffectRef>,
    pub(crate) secondary_power: Option<MobEffectRef>,
    /// Y level reached by the in-progress beam scan; below `pos.y()` restarts the scan.
    last_check_y: i32,
    /// Sections accumulated by the in-progress scan.
    checking_beam_sections: Vec<BeamSection>,
    /// Sections from the last completed scan. Emptiness is the beacon's activity gate.
    pub(crate) beam_sections: Vec<BeamSection>,
}

impl BeaconState {
    const fn new() -> Self {
        Self {
            levels: 0,
            primary_power: None,
            secondary_power: None,
            // Vanilla uses `level.getMinY() - 1`; any value below the beacon restarts the scan.
            last_check_y: i32::MIN,
            checking_beam_sections: Vec::new(),
            beam_sections: Vec::new(),
        }
    }

    pub(crate) fn filter_effect(effect: Option<MobEffectRef>) -> Option<MobEffectRef> {
        effect.filter(|effect| {
            BEACON_EFFECTS
                .iter()
                .copied()
                .flatten()
                .any(|valid| valid.key == effect.key)
        })
    }

    /// Mirrors vanilla `validateEffects`: returns whether the combination is legal for the
    /// given pyramid level.
    pub(crate) fn validate_effects(
        primary: Option<MobEffectRef>,
        secondary: Option<MobEffectRef>,
        levels: i32,
    ) -> bool {
        if secondary.is_some() && levels < MAX_LEVELS {
            return false;
        }
        let primary_level = Self::required_levels_for(primary);
        let secondary_level = Self::required_levels_for(secondary);
        if primary_level > levels || secondary_level > levels {
            return false;
        }
        // Regeneration (tier 4) is secondary-only.
        if primary_level >= MAX_LEVELS {
            return false;
        }
        secondary_level == 0
            || secondary_level >= MAX_LEVELS
            || primary.zip(secondary).is_some_and(|(p, s)| p.key == s.key)
    }

    /// Returns the 1-indexed tier that unlocks `effect`, `0` for `None`, or `i32::MAX` for
    /// effects not in `BEACON_EFFECTS`.
    fn required_levels_for(effect: Option<MobEffectRef>) -> i32 {
        let Some(effect) = effect else {
            return 0;
        };
        for (i, tier) in BEACON_EFFECTS.iter().enumerate() {
            if tier.iter().any(|e| e.key == effect.key) {
                return i as i32 + 1;
            }
        }
        i32::MAX
    }
}

/// Beacon block entity.
pub struct BeaconBlockEntity {
    base: Arc<BlockEntityBase>,
    state: Arc<SyncMutex<BeaconState>>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BeaconBlockEntity`.
unsafe impl DowncastType for BeaconBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/beacon");
}

impl BeaconBlockEntity {
    /// Creates a new beacon block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::BEACON,
            level,
            pos,
            state,
        ));
        Self {
            base,
            state: Arc::new(SyncMutex::new(BeaconState::new())),
        }
    }

    pub(crate) fn state(&self) -> Arc<SyncMutex<BeaconState>> {
        Arc::clone(&self.state)
    }

    /// Returns a handle the menu can use to mark this beacon changed.
    ///
    /// Steel's stand-in for the `ContainerLevelAccess` Vanilla's `BeaconMenu` holds, which it
    /// uses only for `Level::blockEntityChanged`.
    pub(crate) fn base_handle(&self) -> Arc<BlockEntityBase> {
        Arc::clone(&self.base)
    }

    /// Mirrors vanilla `BeaconBlockEntity.playSound`.
    pub(crate) fn play_sound(world: &World, pos: BlockPos, sound: SoundEventRef) {
        world.play_block_sound(sound, pos, 1.0, 1.0, None);
    }

    /// Advances the incremental beam scan by up to [`BLOCKS_CHECK_PER_TICK`] blocks.
    ///
    /// Mirrors the scan loop of vanilla `BeaconBlockEntity.tick`. The beacon block itself is a
    /// beam block (`BeaconBlock implements BeaconBeamBlock`) and is the first position visited,
    /// which seeds the initial section — without it the `last_section.is_none()` guard below
    /// would clear the list on the first air block and no beacon would ever activate.
    fn advance_beam_scan(
        state: &mut BeaconState,
        world: &World,
        pos: BlockPos,
        last_set_block: i32,
    ) {
        let mut check_pos = if state.last_check_y < pos.y() {
            state.checking_beam_sections.clear();
            state.last_check_y = pos.y() - 1;
            pos
        } else {
            BlockPos::new(pos.x(), state.last_check_y + 1, pos.z())
        };

        for _ in 0..BLOCKS_CHECK_PER_TICK {
            if check_pos.y() > last_set_block {
                break;
            }

            let block_state = world.get_block_state(check_pos);
            let beam_color = BLOCK_BEHAVIORS
                .get_behavior(block_state.get_block())
                .beacon_beam_color(block_state);

            if let Some(color) = beam_color {
                let color = ArgbColor::new(color.texture_diffuse_color());
                // Vanilla appends while `size() <= 1`, so the beacon seeds one section and the
                // first tinted block starts a second; only past that do equal colors extend a run.
                if state.checking_beam_sections.len() <= 1 {
                    state.checking_beam_sections.push(BeamSection::new(color));
                } else if let Some(last) = state.checking_beam_sections.last_mut() {
                    if last.color == color {
                        last.increase_height();
                    } else {
                        let blended = last.color.average(color);
                        state.checking_beam_sections.push(BeamSection::new(blended));
                    }
                }
            } else {
                let opaque = block_state.get_block() != &vanilla_blocks::BEDROCK
                    && block_state.get_light_dampening() >= MAX_LIGHT_LEVEL;
                let Some(last) = state.checking_beam_sections.last_mut().filter(|_| !opaque) else {
                    state.checking_beam_sections.clear();
                    state.last_check_y = last_set_block;
                    return;
                };
                last.increase_height();
            }

            check_pos = check_pos.above();
            state.last_check_y += 1;
        }
    }

    /// Recomputes the beacon's pyramid level, mirroring vanilla `updateBase`.
    fn update_base(world: &World, pos: BlockPos) -> i32 {
        let mut levels = 0;
        for step in 1..=MAX_LEVELS {
            let layer_y = pos.y() - step;
            if layer_y < world.get_min_y() {
                break;
            }

            let mut valid = true;
            'outer: for layer_x in (pos.x() - step)..=(pos.x() + step) {
                for layer_z in (pos.z() - step)..=(pos.z() + step) {
                    let state = world.get_block_state(BlockPos::new(layer_x, layer_y, layer_z));
                    if !REGISTRY.blocks.is_in_tag(
                        state.get_block(),
                        &vanilla_block_tags::BlockTag::BEACON_BASE_BLOCKS,
                    ) {
                        valid = false;
                        break 'outer;
                    }
                }
            }

            if !valid {
                break;
            }
            levels = step;
        }
        levels
    }

    fn apply_effects(&self, world: &Arc<World>, pos: BlockPos, levels: i32) {
        let (primary, secondary) = {
            let state = self.state.lock();
            (state.primary_power, state.secondary_power)
        };
        let Some(primary) = primary else {
            return;
        };

        let range = f64::from(levels) * EFFECT_RANGE_PER_LEVEL + BASE_EFFECT_RANGE;
        let base_amplifier = i32::from(
            levels >= MAX_LEVELS && secondary.is_some_and(|s| s.key == primary.key),
        );
        let duration = (9 + levels * 2) * 20;

        // Vanilla: `new AABB(pos).inflate(range).expandTowards(0, level.getHeight(), 0)`.
        let world_height = f64::from(world.get_max_y() - world.get_min_y() + 1);
        let min = DVec3::new(
            f64::from(pos.x()) - range,
            f64::from(pos.y()) - range,
            f64::from(pos.z()) - range,
        );
        let max = DVec3::new(
            f64::from(pos.x()) + 1.0 + range,
            f64::from(pos.y()) + 1.0 + range + world_height,
            f64::from(pos.z()) + 1.0 + range,
        );
        let aabb = WorldAabb::from_min_max(min, max);

        for entity in world.get_entities_in_aabb_matching(&aabb, |entity| {
            entity.entity_type() == &vanilla_entities::PLAYER
        }) {
            let Some(player) = entity.downcast_ref::<Player>() else {
                continue;
            };
            player.add_mob_effect(
                MobEffectInstance::with_duration(primary, duration, base_amplifier)
                    .with_ambient(true)
                    .with_visible(true),
            );

            if let Some(secondary) =
                secondary.filter(|s| levels >= MAX_LEVELS && s.key != primary.key)
            {
                player.add_mob_effect(
                    MobEffectInstance::with_duration(secondary, duration, 0)
                        .with_ambient(true)
                        .with_visible(true),
                );
            }
        }
    }

    fn store_effect(nbt: &mut NbtCompound, field: &str, effect: Option<MobEffectRef>) {
        if let Some(effect) = effect {
            nbt.insert(field, effect.key.to_string());
        }
    }

    fn load_effect(nbt: &BorrowedNbtCompound<'_>, field: &str) -> Option<MobEffectRef> {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let key = Identifier::from_str(&nbt_view.string(field)?.to_string()).ok()?;
        let effect = REGISTRY.mob_effects.by_key(&key)?;
        BeaconState::filter_effect(Some(effect))
    }
}

impl BlockEntity for BeaconBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    // TODO: Persist `CustomName` and `Lock` like Vanilla. Both need foundations Steel lacks: a
    //       block-entity display name and a `LockCode` type.
    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let mut state = self.state.lock();
        state.primary_power = Self::load_effect(nbt, "primary_effect");
        state.secondary_power = Self::load_effect(nbt, "secondary_effect");
    }

    // `Levels` is written but never read back, matching Vanilla; the pyramid is recomputed.
    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        Self::store_effect(nbt, "primary_effect", state.primary_power);
        Self::store_effect(nbt, "secondary_effect", state.secondary_power);
        nbt.insert("Levels", state.levels);
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        {
            let state = self.state.lock();
            Self::store_effect(&mut nbt, "primary_effect", state.primary_power);
            Self::store_effect(&mut nbt, "secondary_effect", state.secondary_power);
            nbt.insert("Levels", state.levels);
        }
        Some(nbt)
    }

    fn tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        // `level_height_at` is already vanilla `Level.getHeight`, so it needs no adjustment.
        let last_set_block = world.level_height_at(HeightmapType::WorldSurface, pos.x(), pos.z());
        let is_interval_tick = world.game_time() % BEACON_TICK_INTERVAL == 0;

        let (previous_levels, levels, had_beam, scan_complete) = {
            let mut state = self.state.lock();
            Self::advance_beam_scan(&mut state, world, pos, last_set_block);

            let previous_levels = state.levels;
            // Read before the swap below: this tick is gated on the last *completed* scan.
            let had_beam = !state.beam_sections.is_empty();
            if is_interval_tick && had_beam {
                state.levels = Self::update_base(world, pos);
            }
            // An obstructed beam leaves `levels` untouched, as Vanilla does. Zeroing it would
            // desync an open menu's data slot and get the client kicked on its next selection.

            let scan_complete = state.last_check_y >= last_set_block;
            if scan_complete {
                state.last_check_y = world.get_min_y() - 1;
                state.beam_sections = mem::take(&mut state.checking_beam_sections);
            }

            (previous_levels, state.levels, had_beam, scan_complete)
        };

        // Outside the state lock: both walk nearby entities and send packets.
        if is_interval_tick && levels > 0 && had_beam {
            self.apply_effects(world, pos, levels);
            Self::play_sound(world, pos, &sound_events::BLOCK_BEACON_AMBIENT);
        }

        if scan_complete {
            // TODO: Trigger the CONSTRUCT_BEACON criterion for nearby players on activation once
            //       Steel's shared advancement foundations exist.
            let sound = match (previous_levels > 0, levels > 0) {
                (false, true) => &sound_events::BLOCK_BEACON_ACTIVATE,
                (true, false) => &sound_events::BLOCK_BEACON_DEACTIVATE,
                _ => return,
            };
            Self::play_sound(world, pos, sound);
        }
    }

    fn on_set_removed(&self) {
        let Some(world) = self.get_level() else {
            return;
        };
        Self::play_sound(
            &world,
            self.get_block_pos(),
            &sound_events::BLOCK_BEACON_DEACTIVATE,
        );
    }
}
