use std::borrow::Cow;

use steel_macros::item_behavior;
use steel_registry::{
    blocks::{BlockRef, properties::Direction},
    data_components::vanilla_components::PROFILE,
    item_stack::ItemStack,
    resolvable_profile::ResolvableProfileContents,
};
use text_components::TextComponent;

use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};

use super::{
    StandingAndWallBlockItem,
    dynamic_name::{default_name, description_id, translated},
};

/// Player-head placement behavior with the profile-specific name.
#[item_behavior]
pub struct PlayerHeadItem {
    #[json_arg(vanilla_blocks, json = "block")]
    _standing_block: BlockRef,
    #[json_arg(vanilla_blocks, json = "wall_block")]
    _wall_block: BlockRef,
    #[json_arg(
        r#enum = "Direction",
        module = "steel_registry::blocks::properties",
        json = "attachment_direction"
    )]
    _attachment_direction: Direction,
    base: StandingAndWallBlockItem,
}

impl PlayerHeadItem {
    /// Creates player-head behavior for the standing and wall block pair.
    #[must_use]
    pub const fn new(
        standing_block: BlockRef,
        wall_block: BlockRef,
        attachment_direction: Direction,
    ) -> Self {
        Self {
            _standing_block: standing_block,
            _wall_block: wall_block,
            _attachment_direction: attachment_direction,
            base: StandingAndWallBlockItem::new(standing_block, wall_block, attachment_direction),
        }
    }
}

impl ItemBehavior for PlayerHeadItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        self.base.use_on(context)
    }

    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        let Some(profile) = stack.get(PROFILE) else {
            return default_name(stack);
        };
        let name = match profile.contents() {
            ResolvableProfileContents::DynamicName(name) => Some(name.as_str()),
            ResolvableProfileContents::StaticFull(profile) => Some(profile.name()),
            ResolvableProfileContents::StaticPartial(profile) => profile.name(),
            ResolvableProfileContents::DynamicId(_) => None,
        };
        let Some(name) = name else {
            return default_name(stack);
        };
        let Some(description_id) = description_id(stack) else {
            return default_name(stack);
        };
        translated(
            format!("{description_id}.named"),
            Some(Box::new([TextComponent::plain(name.to_owned())])),
        )
    }
}
