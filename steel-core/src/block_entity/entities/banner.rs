use std::sync::Weak;

use simdnbt::{
    FromNbtTag, ToNbtTag,
    borrow::{BaseNbtCompound, NbtCompound as NbtView},
    owned::NbtCompound,
};
use steel_registry::{
    DyeColor,
    data_components::{
        DataComponentMap,
        vanilla_components::{BANNER_PATTERNS, BannerPatternLayers, CUSTOM_NAME},
    },
    vanilla_block_entity_types,
};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};
use text_components::{TextComponent, translation::TranslatedMessage};

use crate::{
    block_entity::{BlockEntity, BlockEntityBase, ImplicitComponentGetter},
    nameable::Nameable,
    world::World,
};

struct BannerState {
    name: Option<TextComponent>,
    patterns: BannerPatternLayers,
}

/// Persistent banner patterns and name, with its base color owned by the block.
pub struct BannerBlockEntity {
    base: BlockEntityBase,
    color: DyeColor,
    state: SyncMutex<BannerState>,
}

// SAFETY: This owner-scoped key uniquely identifies Steel's banner implementation.
unsafe impl DowncastType for BannerBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/banner");
}

impl BannerBlockEntity {
    pub(crate) fn from_state(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        use crate::behavior::BLOCK_BEHAVIORS;
        use steel_registry::blocks::block_state_ext::BlockStateExt;
        let Some(color) = BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .banner_color()
        else {
            panic!("registered banner block has no base-color capability");
        };
        Self::new(level, pos, state, color)
    }

    /// Creates a banner using its block's extracted base color.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId, color: DyeColor) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::BANNER, level, pos, state),
            color,
            state: SyncMutex::new(BannerState {
                name: None,
                patterns: BannerPatternLayers::empty(),
            }),
        }
    }

    /// Color of the banner cloth before patterns are applied.
    pub const fn base_color(&self) -> DyeColor {
        self.color
    }
}

impl BlockEntity for BannerBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BaseNbtCompound<'_>) {
        let nbt: NbtView<'_, '_> = nbt.into();
        let mut state = self.state.lock();
        state.name = nbt
            .get("CustomName")
            .and_then(|tag| TextComponent::from_nbt(&tag.to_owned()));
        state.patterns = nbt
            .get("patterns")
            .and_then(BannerPatternLayers::from_nbt_tag)
            .unwrap_or_else(BannerPatternLayers::empty);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        if !state.patterns.layers().is_empty() {
            nbt.insert("patterns", state.patterns.clone().to_nbt_tag());
        }
        if let Some(name) = &state.name {
            nbt.insert("CustomName", name.to_codec_nbt());
        }
    }

    fn apply_implicit_components(&self, components: &mut ImplicitComponentGetter<'_>) {
        let mut state = self.state.lock();
        state.patterns = components
            .get(BANNER_PATTERNS)
            .unwrap_or_else(BannerPatternLayers::empty);
        state.name = components.get(CUSTOM_NAME);
    }

    fn collect_implicit_components(&self, components: &mut DataComponentMap) {
        let state = self.state.lock();
        components.set(BANNER_PATTERNS, Some(state.patterns.clone()));
        components.set(CUSTOM_NAME, state.name.clone());
    }

    fn as_nameable(&self) -> Option<&dyn Nameable> {
        Some(self)
    }
}

impl Nameable for BannerBlockEntity {
    fn custom_name(&self) -> Option<TextComponent> {
        self.state.lock().name.clone()
    }
    fn name(&self) -> TextComponent {
        self.custom_name().unwrap_or_else(|| {
            #[expect(clippy::disallowed_methods, reason = "vanilla BannerBlockEntity uses this key although the target language assets omit it")]
            let message = TranslatedMessage::new(
                "block.minecraft.banner",
                None,
            );
            TextComponent::translated(message)
        })
    }
}
