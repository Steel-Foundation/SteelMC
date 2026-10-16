use crate::behavior::blocks::SkullData;
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use simdnbt::{FromNbtTag, ToNbtTag};
use std::sync::Weak;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::data_components::DataComponentPatch;
use steel_registry::data_components::vanilla_components::{CUSTOM_NAME, NOTE_BLOCK_SOUND, PROFILE};
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    REGISTRY, RegistryEntry, ResolvableProfile, vanilla_block_entity_types, vanilla_blocks,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier};
use text_components::TextComponent;

const PROFILE_NBT_KEY: &str = "profile";
const NOTE_BLOCK_SOUND_NBT_KEY: &str = "note_block_sound";
const CUSTOM_NAME_NBT_KEY: &str = "custom_name";

/// Skull block entity.
///
/// Stores player profile, note block sound and custom name
pub struct SkullBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<SkullData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SkullBlockEntity`.
unsafe impl DowncastType for SkullBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/skull");
}

impl SkullBlockEntity {
    /// Creates a Skull block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::SKULL, level, pos, state),
            state: SyncMutex::new(SkullData::default()),
        }
    }

    /// Used to get the sound identifier by noteblocks.
    pub fn get_note_block_sound(&self) -> Option<Identifier> {
        self.state.lock().note_block_sound.clone()
    }

    /// Builds the picked item form of a placed skull with all stored components.
    pub fn skull_as_picked_item(&self, state: BlockStateId) -> ItemStack {
        let patch = self.state.lock().component_patch();
        Self::skull_as_item(state, patch)
    }

    /// Builds the dropped item form of a placed skull. Only player heads keep their profile and
    /// note block sound.
    pub fn skull_as_dropped_item(&self, state: BlockStateId) -> ItemStack {
        let block = state.get_block();
        let is_player_head = block.key() == vanilla_blocks::PLAYER_HEAD.key()
            || block.key() == vanilla_blocks::PLAYER_WALL_HEAD.key();
        let data = self.state.lock();
        let patch = if is_player_head {
            data.component_patch()
        } else {
            data.custom_name_patch()
        };
        drop(data);
        Self::skull_as_item(state, patch)
    }

    fn skull_as_item(state: BlockStateId, patch: DataComponentPatch) -> ItemStack {
        let block_item = REGISTRY.items.by_block(state.get_block());
        ItemStack::with_count_and_patch(block_item, 1, patch)
    }
}

impl BlockEntity for SkullBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let mut state = self.state.lock();

        state.owner =
            ResolvableProfile::from_optional_nbt_tag(nbt.get(PROFILE_NBT_KEY)).unwrap_or_default();
        state.note_block_sound =
            Identifier::from_optional_nbt_tag(nbt.get(NOTE_BLOCK_SOUND_NBT_KEY))
                .unwrap_or_default();
        state.custom_name =
            TextComponent::from_optional_nbt_tag(nbt.get(CUSTOM_NAME_NBT_KEY)).unwrap_or_default();
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();

        if let Some(profile) = state.owner.clone().to_optional_nbt_tag() {
            nbt.insert(PROFILE_NBT_KEY, profile);
        }

        if let Some(sound) = state.note_block_sound.clone().to_optional_nbt_tag() {
            nbt.insert(NOTE_BLOCK_SOUND_NBT_KEY, sound);
        }

        if let Some(name) = state.custom_name.clone().to_optional_nbt_tag() {
            nbt.insert(CUSTOM_NAME_NBT_KEY, name);
        }
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        Some(self.save_custom_only())
    }

    fn apply_components_from_item(&self, item: &ItemStack) {
        let mut state = self.state.lock();
        state.owner = item.get(PROFILE).cloned();
        state.note_block_sound = item.get(NOTE_BLOCK_SOUND).cloned();
        state.custom_name = item.get(CUSTOM_NAME).cloned();
    }
}
