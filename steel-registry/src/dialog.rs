use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
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

impl ToNbtTag for &Dialog {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        compound.insert(
            "type",
            match &self.variant {
                DialogVariant::DialogList { .. } => "minecraft:dialog_list",
                DialogVariant::ServerLinks => "minecraft:server_links",
            },
        );
        compound.insert("title", (&self.title).to_nbt_tag());
        compound.insert("external_title", (&self.external_title).to_nbt_tag());
        compound.insert("button_width", self.button_width);
        compound.insert("columns", self.columns);
        let mut exit_action = NbtCompound::new();
        exit_action.insert("label", (&self.exit_action.label).to_nbt_tag());
        exit_action.insert("width", self.exit_action.width);
        compound.insert("exit_action", NbtTag::Compound(exit_action));
        if let DialogVariant::DialogList { dialogs } = &self.variant {
            compound.insert("dialogs", *dialogs);
        }
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(
    DialogRegistry,
    Dialog,
    stem: dialogs,
    tagged: "dialog",
);
