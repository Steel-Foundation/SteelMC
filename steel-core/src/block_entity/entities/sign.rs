//! Sign block entity implementation.
//!
//! Signs store text on both front and back sides, along with color and glow
//! information.

use std::any::Any;
use std::array;
use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{
    BaseNbtCompound as BorrowedNbtCompound, NbtCompound as BorrowedNbtCompoundView,
};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::loot_table::DyeColor;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId};
use text_components::{TextComponent, content::Content};
use uuid::Uuid;

use crate::block_entity::BlockEntity;
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
        // Load messages - they are stored as a list of compounds (text components)
        if let Some(messages) = nbt.list("messages")
            && let Some(compounds) = messages.compounds()
        {
            for (i, compound) in compounds.into_iter().enumerate().take(SIGN_LINES) {
                if let Some(text) = TextComponent::from_nbt(&NbtTag::Compound(compound.to_owned()))
                {
                    self.messages[i] = text;
                }
            }
        }

        // Load color
        if let Some(color_str) = nbt.string("color") {
            self.color = dye_color_from_str(&color_str.to_str());
        }

        // Load glow
        if let Some(glow) = nbt.byte("has_glowing_text") {
            self.has_glowing_text = glow != 0;
        }
    }

    /// Saves sign text to NBT.
    pub fn save(&self, nbt: &mut NbtCompound) {
        // Save messages as a list of compounds (text components)
        let compounds: Vec<NbtCompound> = self
            .messages
            .iter()
            .map(|msg| msg.to_nbt_tag().into_compound().unwrap_or_default())
            .collect();
        nbt.insert("messages", NbtList::Compound(compounds));

        // Save color
        nbt.insert("color", dye_color_to_str(self.color));

        // Save glow
        nbt.insert("has_glowing_text", i8::from(self.has_glowing_text));
    }
}

/// Sign block entity.
///
/// Stores text on both front and back sides of the sign.
pub struct SignBlockEntity {
    /// Weak reference to the world for marking chunks dirty.
    level: Weak<World>,
    /// Block entity type (sign or `hanging_sign`).
    block_entity_type: BlockEntityTypeRef,
    /// Position in the world.
    pos: BlockPos,
    /// Current block state.
    state: BlockStateId,
    /// Whether this entity has been marked for removal.
    removed: bool,
    /// Text on the front side.
    pub front_text: SignText,
    /// Text on the back side.
    pub back_text: SignText,
    /// Whether the sign is waxed (prevents editing).
    pub is_waxed: bool,
    /// UUID of the player currently allowed to edit this sign.
    /// Used to prevent multiple players from editing simultaneously.
    player_who_may_edit: Option<Uuid>,
}

impl SignBlockEntity {
    /// Creates a new sign block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::with_type(level, vanilla_block_entity_types::SIGN, pos, state)
    }

    /// Creates a new hanging sign block entity.
    #[must_use]
    pub fn new_hanging(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::with_type(level, vanilla_block_entity_types::HANGING_SIGN, pos, state)
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
            level,
            block_entity_type,
            pos,
            state,
            removed: false,
            front_text: SignText::new(),
            back_text: SignText::new(),
            is_waxed: false,
            player_who_may_edit: None,
        }
    }

    /// Gets the UUID of the player currently allowed to edit this sign.
    #[must_use]
    pub const fn get_player_who_may_edit(&self) -> Option<Uuid> {
        self.player_who_may_edit
    }

    /// Sets the player allowed to edit this sign.
    pub const fn set_player_who_may_edit(&mut self, player: Option<Uuid>) {
        self.player_who_may_edit = player;
    }

    /// Checks if another player (not the given one) is currently editing this sign.
    #[must_use]
    pub fn is_other_player_editing(&self, player_uuid: Uuid) -> bool {
        self.player_who_may_edit
            .is_some_and(|editor| editor != player_uuid)
    }

    /// Gets the text for a side.
    #[must_use]
    pub const fn get_text(&self, front: bool) -> &SignText {
        if front {
            &self.front_text
        } else {
            &self.back_text
        }
    }

    /// Gets mutable text for a side.
    pub const fn get_text_mut(&mut self, front: bool) -> &mut SignText {
        if front {
            &mut self.front_text
        } else {
            &mut self.back_text
        }
    }

    /// Sets the text for a side.
    pub fn set_text(&mut self, text: SignText, front: bool) {
        if front {
            self.front_text = text;
        } else {
            self.back_text = text;
        }
    }
}

impl BlockEntity for SignBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> BlockEntityTypeRef {
        self.block_entity_type
    }

    fn get_block_pos(&self) -> BlockPos {
        self.pos
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to NbtCompound view for accessing methods
        let nbt_view: BorrowedNbtCompoundView<'_, '_> = nbt.into();

        // Load front text
        if let Some(front_nbt) = nbt_view.compound("front_text") {
            self.front_text.load(front_nbt);
        }

        // Load back text
        if let Some(back_nbt) = nbt_view.compound("back_text") {
            self.back_text.load(back_nbt);
        }

        // Load waxed state
        if let Some(waxed) = nbt_view.byte("is_waxed") {
            self.is_waxed = waxed != 0;
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Save front text
        let mut front_nbt = NbtCompound::new();
        self.front_text.save(&mut front_nbt);
        nbt.insert("front_text", front_nbt);

        // Save back text
        let mut back_nbt = NbtCompound::new();
        self.back_text.save(&mut back_nbt);
        nbt.insert("back_text", back_nbt);

        // Save waxed state
        nbt.insert("is_waxed", i8::from(self.is_waxed));
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Send full sign data to client
        let mut nbt = NbtCompound::new();
        self.save_additional(&mut nbt);
        Some(nbt)
    }

    fn is_ticking(&self) -> bool {
        // Signs tick to clear the edit lock if the player moves away
        self.player_who_may_edit.is_some()
    }

    fn tick(&mut self, world: &World) {
        // Clear the edit lock if the editing player is too far away or gone
        if let Some(editor_uuid) = self.player_who_may_edit {
            let should_clear = world
                .players
                .get_by_uuid(&editor_uuid)
                .is_none_or(|player| {
                    let pos = self.pos;
                    let player_pos = *player.position.lock();
                    let dx = player_pos.x - f64::from(pos.0.x) - 0.5;
                    let dy = player_pos.y - f64::from(pos.0.y) - 0.5;
                    let dz = player_pos.z - f64::from(pos.0.z) - 0.5;
                    let distance_sq = dx * dx + dy * dy + dz * dz;
                    distance_sq > MAX_EDIT_DISTANCE * MAX_EDIT_DISTANCE
                });

            if should_clear {
                self.player_who_may_edit = None;
            }
        }
    }
}

/// Converts a dye color to its string representation.
const fn dye_color_to_str(color: DyeColor) -> &'static str {
    match color {
        DyeColor::White => "white",
        DyeColor::Orange => "orange",
        DyeColor::Magenta => "magenta",
        DyeColor::LightBlue => "light_blue",
        DyeColor::Yellow => "yellow",
        DyeColor::Lime => "lime",
        DyeColor::Pink => "pink",
        DyeColor::Gray => "gray",
        DyeColor::LightGray => "light_gray",
        DyeColor::Cyan => "cyan",
        DyeColor::Purple => "purple",
        DyeColor::Blue => "blue",
        DyeColor::Brown => "brown",
        DyeColor::Green => "green",
        DyeColor::Red => "red",
        DyeColor::Black => "black",
    }
}

/// Parses a dye color from its string representation.
fn dye_color_from_str(s: &str) -> DyeColor {
    match s {
        "white" => DyeColor::White,
        "orange" => DyeColor::Orange,
        "magenta" => DyeColor::Magenta,
        "light_blue" => DyeColor::LightBlue,
        "yellow" => DyeColor::Yellow,
        "lime" => DyeColor::Lime,
        "pink" => DyeColor::Pink,
        "gray" => DyeColor::Gray,
        "light_gray" => DyeColor::LightGray,
        "cyan" => DyeColor::Cyan,
        "purple" => DyeColor::Purple,
        "blue" => DyeColor::Blue,
        "brown" => DyeColor::Brown,
        "green" => DyeColor::Green,
        "red" => DyeColor::Red,
        _ => DyeColor::Black,
    }
}
