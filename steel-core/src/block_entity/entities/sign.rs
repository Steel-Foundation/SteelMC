//! Sign block entity implementation.
//!
//! Signs store text on both front and back sides, along with color and glow
//! information.

use std::array;
use std::sync::{Arc, Weak};

use simdnbt::borrow::{
    BaseNbtCompound as BorrowedNbtCompound, NbtCompound as BorrowedNbtCompoundView,
};
use simdnbt::owned::{NbtCompound, NbtList};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::{DyeColor, vanilla_block_entity_types};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};
use text_components::{TextComponent, content::Content};
use uuid::Uuid;

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::Entity;
use crate::world::World;

/// Maximum distance (in blocks) a player can be from a sign while editing.
/// If they move further away, the edit lock is released.
const MAX_EDIT_DISTANCE: f64 = 4.0;

/// Number of text lines on each side of a sign.
pub const SIGN_LINES: usize = 4;

/// Text and styling for one side of a sign.
#[derive(Debug, Clone)]
pub struct SignText {
    /// The 4 lines of text (raw, unfiltered).
    pub messages: [TextComponent; SIGN_LINES],
    /// Text color (dye color applied to the sign).
    pub color: DyeColor,
    /// Whether the text has a glowing effect (from glow ink sac).
    pub has_glowing_text: bool,
}

impl Default for SignText {
    fn default() -> Self {
        Self::new()
    }
}

impl SignText {
    /// Creates a new empty sign text with default color (black) and no glow.
    #[must_use]
    pub fn new() -> Self {
        Self {
            messages: array::from_fn(|_| TextComponent::new()),
            color: DyeColor::Black,
            has_glowing_text: false,
        }
    }

    /// Gets a message line by index.
    #[must_use]
    pub fn get_message(&self, index: usize) -> Option<&TextComponent> {
        self.messages.get(index)
    }

    /// Sets a message line by index.
    pub fn set_message(&mut self, index: usize, message: TextComponent) {
        if index < SIGN_LINES {
            self.messages[index] = message;
        }
    }

    /// Checks if any line has text content.
    #[must_use]
    pub fn has_message(&self) -> bool {
        self.messages.iter().any(|msg| {
            // Check if the text component has any actual content
            match &msg.content {
                Content::Text { text } => !text.is_empty(),
                _ => true, // Translations, etc. count as having a message
            }
        })
    }

    /// Loads sign text from borrowed NBT.
    pub fn load(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(messages) = nbt.list("messages") {
            let tags = messages.to_owned().as_nbt_tags();
            let messages = tags
                .iter()
                .map(TextComponent::from_nbt)
                .collect::<Option<Vec<_>>>();
            if let Some(messages) = messages
                && let Ok(messages) = <[TextComponent; SIGN_LINES]>::try_from(messages)
            {
                self.messages = messages;
            }
        }

        // Load color
        if let Some(color_str) = nbt.string("color") {
            self.color =
                DyeColor::from_serialized_name(&color_str.to_str()).unwrap_or(DyeColor::Black);
        }

        // Load glow
        if let Some(glow) = nbt.byte("has_glowing_text") {
            self.has_glowing_text = glow != 0;
        }
    }

    /// Saves sign text to NBT.
    pub fn save(&self, nbt: &mut NbtCompound) {
        nbt.insert(
            "messages",
            NbtList::from(
                self.messages
                    .iter()
                    .map(TextComponent::to_codec_nbt)
                    .collect::<Vec<_>>(),
            ),
        );

        // Save color
        nbt.insert("color", self.color.serialized_name());

        // Save glow
        nbt.insert("has_glowing_text", i8::from(self.has_glowing_text));
    }
}

/// Sign block entity.
///
/// Stores text on both front and back sides of the sign.
pub struct SignBlockEntity {
    base: BlockEntityBase,
    sign: SyncMutex<SignState>,
}

struct SignState {
    /// Text on the front side.
    front_text: SignText,
    /// Text on the back side.
    back_text: SignText,
    /// Whether the sign is waxed (prevents editing).
    is_waxed: bool,
    /// UUID of the player currently allowed to edit this sign.
    /// Used to prevent multiple players from editing simultaneously.
    player_who_may_edit: Option<Uuid>,
}

// SAFETY: This key identifies Steel's shared sign implementation for both sign
// registry entries, rather than either registry entry itself.
unsafe impl DowncastType for SignBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/sign");
}

impl SignBlockEntity {
    /// Creates a new sign block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::with_type(level, &vanilla_block_entity_types::SIGN, pos, state)
    }

    /// Creates a new hanging sign block entity.
    #[must_use]
    pub fn new_hanging(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::with_type(level, &vanilla_block_entity_types::HANGING_SIGN, pos, state)
    }

    /// Creates a sign block entity with a specific type.
    #[must_use]
    pub fn with_type(
        level: Weak<World>,
        block_entity_type: BlockEntityTypeRef,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        Self {
            base: BlockEntityBase::new(block_entity_type, level, pos, state),
            sign: SyncMutex::new(SignState {
                front_text: SignText::new(),
                back_text: SignText::new(),
                is_waxed: false,
                player_who_may_edit: None,
            }),
        }
    }

    /// Gets the UUID of the player currently allowed to edit this sign.
    #[must_use]
    pub fn get_player_who_may_edit(&self) -> Option<Uuid> {
        self.sign.lock().player_who_may_edit
    }

    /// Sets the player allowed to edit this sign.
    pub fn set_player_who_may_edit(&self, player: Option<Uuid>) {
        self.sign.lock().player_who_may_edit = player;
    }

    /// Checks if another player (not the given one) is currently editing this sign.
    #[must_use]
    pub fn is_other_player_editing(&self, player_uuid: Uuid) -> bool {
        self.sign
            .lock()
            .player_who_may_edit
            .is_some_and(|editor| editor != player_uuid)
    }

    /// Gets the text for a side.
    #[must_use]
    pub fn get_text(&self, front: bool) -> SignText {
        let sign = self.sign.lock();
        if front {
            sign.front_text.clone()
        } else {
            sign.back_text.clone()
        }
    }

    /// Returns whether this sign is waxed.
    #[must_use]
    pub fn is_waxed(&self) -> bool {
        self.sign.lock().is_waxed
    }

    /// Makes this sign waxed, returning whether its state changed.
    pub fn wax(&self) -> bool {
        let mut sign = self.sign.lock();
        if sign.is_waxed {
            return false;
        }
        sign.is_waxed = true;
        true
    }

    /// Sets the text for a side.
    pub fn set_text(&self, text: SignText, front: bool) {
        let mut sign = self.sign.lock();
        if front {
            sign.front_text = text;
        } else {
            sign.back_text = text;
        }
    }
}

impl BlockEntity for SignBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to NbtCompound view for accessing methods
        let nbt_view: BorrowedNbtCompoundView<'_, '_> = nbt.into();
        let mut sign = self.sign.lock();

        // Load front text
        if let Some(front_nbt) = nbt_view.compound("front_text") {
            sign.front_text.load(front_nbt);
        }

        // Load back text
        if let Some(back_nbt) = nbt_view.compound("back_text") {
            sign.back_text.load(back_nbt);
        }

        // Load waxed state
        if let Some(waxed) = nbt_view.byte("is_waxed") {
            sign.is_waxed = waxed != 0;
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let sign = self.sign.lock();
        // Save front text
        let mut front_nbt = NbtCompound::new();
        sign.front_text.save(&mut front_nbt);
        nbt.insert("front_text", front_nbt);

        // Save back text
        let mut back_nbt = NbtCompound::new();
        sign.back_text.save(&mut back_nbt);
        nbt.insert("back_text", back_nbt);

        // Save waxed state
        nbt.insert("is_waxed", i8::from(sign.is_waxed));
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Send full sign data to client
        let mut nbt = NbtCompound::new();
        self.save_additional(&mut nbt);
        Some(nbt)
    }

    fn tick(&self, world: &Arc<World>) {
        // Clear the edit lock if the editing player is too far away or gone
        let editor_uuid = self.sign.lock().player_who_may_edit;
        let Some(editor_uuid) = editor_uuid else {
            return;
        };
        let should_clear = world
            .players
            .get_by_uuid(&editor_uuid)
            .is_none_or(|player| {
                let pos = self.get_block_pos();
                let player_pos = player.position();
                let dx = player_pos.x - f64::from(pos.0.x) - 0.5;
                let dy = player_pos.y - f64::from(pos.0.y) - 0.5;
                let dz = player_pos.z - f64::from(pos.0.z) - 0.5;
                let distance_sq = dx * dx + dy * dy + dz * dz;
                distance_sq > MAX_EDIT_DISTANCE * MAX_EDIT_DISTANCE
            });

        if should_clear {
            let mut sign = self.sign.lock();
            if sign.player_who_may_edit == Some(editor_uuid) {
                sign.player_who_may_edit = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{array, io::Cursor, sync::Arc};

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::BlockPos;
    use text_components::{Modifier as _, TextComponent};
    use uuid::Uuid;

    use super::{SignBlockEntity, SignText};
    use crate::block_entity::BlockEntity as _;
    use crate::test_support::fresh_test_world;

    #[test]
    fn plain_sign_lines_save_as_a_string_list() {
        let mut text = SignText::new();
        text.messages = array::from_fn(|index| TextComponent::plain(index.to_string()));

        let mut nbt = NbtCompound::new();
        text.save(&mut nbt);

        assert_eq!(
            nbt.get("messages"),
            Some(&NbtTag::List(NbtList::String(vec![
                "0".into(),
                "1".into(),
                "2".into(),
                "3".into(),
            ])))
        );
    }

    #[test]
    fn mixed_sign_lines_round_trip_through_the_component_codec() {
        let mut expected = SignText::new();
        expected.messages[0] = TextComponent::plain("plain");
        expected.messages[1] = TextComponent::plain("styled").bold(true);

        let mut nbt = NbtCompound::new();
        expected.save(&mut nbt);
        assert!(matches!(
            nbt.get("messages"),
            Some(NbtTag::List(NbtList::Compound(_)))
        ));

        let mut bytes = Vec::new();
        NbtTag::Compound(nbt).write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice()))
            .expect("saved sign text should be valid NBT");
        let borrowed_tag = borrowed.as_tag();
        let compound = borrowed_tag
            .compound()
            .expect("saved sign text should be a compound");

        let mut decoded = SignText::new();
        decoded.load(compound);

        assert_eq!(decoded.messages, expected.messages);
        assert_eq!(decoded.color, expected.color);
        assert_eq!(decoded.has_glowing_text, expected.has_glowing_text);
    }

    #[test]
    fn sign_tick_releases_state_before_player_lookup_and_editor_clear() {
        init_test_registry();
        let world = fresh_test_world("sign_editor_clear");
        let sign = SignBlockEntity::new(
            Arc::downgrade(&world),
            BlockPos::new(8, 64, 8),
            vanilla_blocks::OAK_SIGN.default_state(),
        );
        sign.set_player_who_may_edit(Some(Uuid::from_u128(1)));

        sign.tick(&world);
        assert_eq!(sign.get_player_who_may_edit(), None);
    }
}
