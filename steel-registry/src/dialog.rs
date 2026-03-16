use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use text_components::TextComponent;

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
    tags: FxHashMap<Identifier, Vec<Identifier>>,
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

    /// Replaces a dialog at a given index.
    /// Returns true if the dialog was replaced and false if the dialog wasn't replaced
    #[must_use]
    pub fn replace(&mut self, dialog: DialogRef, id: usize) -> bool {
        if id >= self.dialogs_by_id.len() {
            return false;
        }
        self.dialogs_by_id[id] = dialog;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, DialogRef)> + '_ {
        self.dialogs_by_id
            .iter()
            .enumerate()
            .map(|(id, &dialog)| (id, dialog))
    }
}

crate::impl_registry!(
    DialogRegistry,
    Dialog,
    dialogs_by_id,
    dialogs_by_key,
    dialogs
);
crate::impl_tagged_registry!(DialogRegistry, dialogs_by_key, "dialog");

impl Default for DialogRegistry {
    fn default() -> Self {
        Self::new()
    }
}
