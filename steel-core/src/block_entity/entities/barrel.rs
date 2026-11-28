//! Barrel block entity.
use crate::{
    block_entity::{
        BlockEntity, BlockEntityBase, BlockEntityLifecycleExt as _, ImplicitComponentGetter,
        RandomizableContainer, container_openers_counter::ContainerOpenersCounter,
        container_properties::ContainerProperties,
        randomizable_container::RandomizableContainerStorage,
    },
    entity::{ContainerUser, Entity},
    inventory::lock::ContainerRef,
    loot::LootResult,
    nameable::Nameable,
    player::Player,
    world::{World, game_event::GameEventContext},
};
use glam::DVec3;
use simdnbt::{borrow::BaseNbtCompound, owned::NbtCompound};
use std::sync::{Arc, Weak};
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt as _, properties::BlockStateProperties},
    data_components::vanilla_components::SeededContainerLoot,
    sound_events, vanilla_block_entity_types, vanilla_game_events,
};
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, translations, types::UpdateFlags,
};
use text_components::TextComponent;

/// Number of slots in a barrel.
pub const BARREL_SLOTS: usize = 27;
const SOUND_FACE_OFFSET: f64 = 0.5;
const SOUND_VOLUME: f32 = 0.5;
const SOUND_MIN_PITCH: f32 = 0.9;
const SOUND_PITCH_VARIATION: f32 = 0.1;

/// A barrel's independent inventory and lazy loot storage.
pub struct BarrelBlockEntity {
    storage: RandomizableContainerStorage,
    properties: ContainerProperties,
    openers: ContainerOpenersCounter,
}

// SAFETY: This Steel-owned key uniquely identifies `BarrelBlockEntity`.
unsafe impl DowncastType for BarrelBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/barrel");
}

impl BarrelBlockEntity {
    /// Creates a barrel with independent inventory storage.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            storage: RandomizableContainerStorage::new(
                &vanilla_block_entity_types::BARREL,
                BARREL_SLOTS,
                level,
                pos,
                state,
            ),
            properties: ContainerProperties::default(),
            openers: ContainerOpenersCounter::default(),
        }
    }

    /// Checks the lock and the spectator restriction on unopened loot.
    pub(crate) fn can_open(&self, player: &Player) -> bool {
        (!player.is_spectator() || self.loot_table().is_none()) && self.properties.can_open(player)
    }

    /// Reconciles the barrel's open state with its live viewers.
    pub(crate) fn recheck_open(&self) {
        if self.is_removed() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };

        let change = self.openers.recheck(
            &world,
            self.get_block_pos(),
            self.storage.container_ref.container_id(),
        );
        let is_open = change.current != 0;
        let was_open = change.previous != 0;

        if was_open != is_open {
            self.set_open(&world, is_open, None);
        }
        if change.current > 0 {
            self.schedule_recheck(&world);
        }
    }

    fn schedule_recheck(&self, world: &World) {
        world.schedule_block_tick_default(
            self.get_block_pos(),
            self.get_block_state().get_block(),
            ContainerOpenersCounter::CHECK_TICK_DELAY,
        );
    }

    fn set_open(&self, world: &Arc<World>, open: bool, source: Option<&dyn Entity>) {
        let pos = self.get_block_pos();
        let state = self.get_block_state();
        let facing = state.get_value(&BlockStateProperties::FACING);
        let sound_pos = pos.get_center() + DVec3::from(facing.offset_vec()) * SOUND_FACE_OFFSET;

        let (sound, event) = if open {
            (
                &sound_events::BLOCK_BARREL_OPEN,
                &vanilla_game_events::CONTAINER_OPEN,
            )
        } else {
            (
                &sound_events::BLOCK_BARREL_CLOSE,
                &vanilla_game_events::CONTAINER_CLOSE,
            )
        };

        world.play_sound_at(
            sound,
            SoundSource::Blocks,
            sound_pos,
            SOUND_VOLUME,
            rand::random::<f32>() * SOUND_PITCH_VARIATION + SOUND_MIN_PITCH,
            None,
        );
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::OPEN, open),
            UpdateFlags::UPDATE_ALL,
        );
        world.game_event(event, pos, &GameEventContext::new(source, None));
    }
}

impl BlockEntity for BarrelBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.storage.base
    }

    fn load_additional(&self, nbt: &BaseNbtCompound<'_>) {
        self.properties.load(nbt);
        self.storage.load_additional(nbt);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.properties.save(nbt);
        self.storage.save_additional(nbt);
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        self.storage.pre_remove_side_effects(pos);
    }

    fn apply_implicit_components(&self, components: &mut ImplicitComponentGetter<'_>) {
        self.properties.apply_components(components);
        self.storage.apply_implicit_components(components);
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.storage.container_ref.clone())
    }

    fn as_nameable(&self) -> Option<&dyn Nameable> {
        Some(self)
    }

    fn as_randomizable_container(&self) -> Option<&dyn RandomizableContainer> {
        Some(self)
    }

    fn start_open(&self, user: &dyn ContainerUser) {
        if self.is_removed() || user.is_spectator() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };

        if self.openers.increment(user).previous == 0 {
            self.set_open(&world, true, Some(user));
            self.schedule_recheck(&world);
        }
    }

    fn stop_open(&self, user: &dyn ContainerUser) {
        if self.is_removed() || user.is_spectator() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };

        if self.openers.decrement().current == 0 {
            self.set_open(&world, false, Some(user));
        }
    }
}

impl RandomizableContainer for BarrelBlockEntity {
    fn loot_table(&self) -> Option<SeededContainerLoot> {
        self.storage.loot_table()
    }

    fn set_loot_table(&self, loot: Option<SeededContainerLoot>) {
        self.storage.set_loot_table(loot);
    }

    fn unpack_loot_table(&self, player: Option<&Player>) -> LootResult<()> {
        self.storage.unpack_loot_table(player)
    }
}

impl Nameable for BarrelBlockEntity {
    fn custom_name(&self) -> Option<TextComponent> {
        self.properties.custom_name()
    }

    fn name(&self) -> TextComponent {
        self.properties
            .custom_name()
            .unwrap_or_else(|| translations::CONTAINER_BARREL.msg().component())
    }
}

#[cfg(test)]
#[path = "barrel/tests.rs"]
mod tests;
