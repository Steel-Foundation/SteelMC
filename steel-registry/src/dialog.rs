use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use text_components::TextComponent;

use crate::RegistryExt;

/// Represents a dialog defined in data packs.
#[derive(Debug)]
pub struct Dialog {
    pub key: Identifier,
    pub button_width: i32,
    pub columns: i32,
    pub exit_action: ExitAction,
    pub external_title: TextComponent,
    pub title: TextComponent,
    pub variant: DialogVariant,
}

/// The variant-specific data for a dialog.
#[derive(Debug)]
pub enum DialogVariant {
    /// A dialog that displays a list of other dialogs.
    DialogList { dialogs: &'static str },
    /// A dialog that displays server links.
    ServerLinks,
}

/// Represents an exit action with a label and width.
#[derive(Debug)]
pub struct ExitAction {
    pub label: TextComponent,
    pub width: i32,
}

pub type DialogRef = &'static Dialog;

pub struct DialogRegistry {
    dialogs_by_id: Vec<DialogRef>,
    dialogs_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<DialogRef>>,
    allows_registering: bool,
}

impl DialogRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            dialogs_by_id: Vec::new(),
            dialogs_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, dialog: DialogRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register dialogs after the registry has been frozen"
        );

        let id = self.dialogs_by_id.len();
        self.dialogs_by_key.insert(dialog.key.clone(), id);
        self.dialogs_by_id.push(dialog);
        id
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<DialogRef> {
        self.dialogs_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, dialog: DialogRef) -> &usize {
        self.dialogs_by_key
            .get(&dialog.key)
            .expect("Dialog not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<DialogRef> {
        self.dialogs_by_key.get(key).and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, DialogRef)> + '_ {
        self.dialogs_by_id
            .iter()
            .enumerate()
            .map(|(id, &dialog)| (id, dialog))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.dialogs_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dialogs_by_id.is_empty()
    }

    /// Registers a tag with a list of dialog keys.
    /// Dialog keys that don't exist in the registry are silently skipped.
    pub fn register_tag(&mut self, tag: Identifier, dialog_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let dialogs: Vec<DialogRef> = dialog_keys
            .iter()
            .filter_map(|key| self.by_key(&Identifier::vanilla_static(key)))
            .collect();

        self.tags.insert(tag, dialogs);
    }

    /// Checks if a dialog is in a given tag.
    #[must_use]
    pub fn is_in_tag(&self, dialog: DialogRef, tag: &Identifier) -> bool {
        self.tags.get(tag).is_some_and(|dialogs| {
            dialogs
                .iter()
                .any(|&d| std::ptr::eq(std::ptr::from_ref(d), std::ptr::from_ref(dialog)))
        })
    }

    /// Gets all dialogs in a tag.
    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<&[DialogRef]> {
        self.tags.get(tag).map(std::vec::Vec::as_slice)
    }

    /// Iterates over all dialogs in a tag.
    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = DialogRef> + '_ {
        self.tags
            .get(tag)
            .map(|v| v.iter().copied())
            .into_iter()
            .flatten()
    }

    /// Gets all tag keys.
    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> + '_ {
        self.tags.keys()
    }
}

impl RegistryExt for DialogRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for DialogRegistry {
    fn default() -> Self {
        Self::new()
    }
}
