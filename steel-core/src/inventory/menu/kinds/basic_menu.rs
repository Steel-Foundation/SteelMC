use crate::inventory::menu::MenuKind;

/// A menu kind with all-default handling and no special behavior.
#[derive(Debug)]
pub struct BasicKind;

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for BasicKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/basic");
}

impl MenuKind for BasicKind {}
