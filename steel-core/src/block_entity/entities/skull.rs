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
    state: SyncMutex<SkullState>,
}

struct SkullState {
    owner: Option<ResolvableProfile>,
    note_block_sound: Option<Identifier>,
    custom_name: Option<TextComponent>,
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
            state: SyncMutex::new(SkullState {
                owner: None,
                note_block_sound: None,
                custom_name: None,
            }),
        }
    }

    /// Used to get the sound identifier by noteblocks.
    pub fn get_note_block_sound(&self) -> Option<Identifier> {
        self.state.lock().note_block_sound.clone()
    }

    /// Builds the item form of a placed skull possibly with profile, custom name or note block sound.
    /// Vanilla `SkullBlockEntity.collectImplicitComponents()`
    pub fn skull_as_item(&self, state: BlockStateId) -> ItemStack {
        let block_item = REGISTRY.items.by_block(state.get_block());

        let skull_state = self.state.lock();

        let mut patch = DataComponentPatch::new();

        if state.get_block().key() == vanilla_blocks::PLAYER_HEAD.key()
            || state.get_block().key() == vanilla_blocks::PLAYER_WALL_HEAD.key()
        {
            if let Some(p) = skull_state.owner.clone() {
                patch.set(PROFILE, p);
            }
            if let Some(s) = skull_state.note_block_sound.clone() {
                patch.set(NOTE_BLOCK_SOUND, s);
            }
        }

        if let Some(n) = skull_state.custom_name.clone() {
            patch.set(CUSTOM_NAME, n);
        }

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

        if let Ok(profile) = ResolvableProfile::from_optional_nbt_tag(nbt.get(PROFILE_NBT_KEY)) {
            state.owner = profile;
        }
        if let Ok(sound) = Identifier::from_optional_nbt_tag(nbt.get(NOTE_BLOCK_SOUND_NBT_KEY)) {
            state.note_block_sound = sound;
        }
        if let Ok(name) = TextComponent::from_optional_nbt_tag(nbt.get(CUSTOM_NAME_NBT_KEY)) {
            state.custom_name = name;
        }
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
        if let Some(profile) = item.get(PROFILE) {
            state.owner = Some(profile.clone());
        }

        if let Some(sound) = item.get(NOTE_BLOCK_SOUND) {
            state.note_block_sound = Some(sound.clone());
        }

        if let Some(name) = item.get(CUSTOM_NAME) {
            state.custom_name = Some(name.clone());
        }
    }
}
