use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use simdnbt::{FromNbtTag, ToNbtTag};
use std::sync::Weak;
use steel_registry::{ResolvableProfile, vanilla_block_entity_types};
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

    // fn apply_components_from_item(&self, item: &ItemStack) {
    // TODO: Wait for shulkerbox pr to add this function to blockentitiy
    // let Some(contents) = item.get(CONTAINER) else {
    //     return;
    // };
    //
    // let mut container = self.container.lock();
    // container.items.fill(ItemStack::empty());
    // for (slot, template) in contents.items().iter().enumerate() {
    //     if slot >= SHULKER_BOX_SLOTS {
    //         break;
    //     }
    //     if let Some(template) = template {
    //         container.items_mut()[slot] = ItemStack::with_count_and_patch(
    //             template.item(),
    //             template.count(),
    //             template.components().clone(),
    //         );
    //     }
    // }
    // }
}
