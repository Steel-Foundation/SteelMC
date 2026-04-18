use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a menu type (container/GUI type) in Minecraft.
/// Menu types define the different inventory interfaces available,
/// such as chests, furnaces, anvils, etc.
#[derive(Debug)]
pub struct MenuType {
    pub key: Identifier,
}

pub type MenuTypeRef = &'static MenuType;

pub struct MenuTypeRegistry {
    menu_types_by_id: Vec<MenuTypeRef>,
    menu_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl MenuTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            menu_types_by_id: Vec::new(),
            menu_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    MenuTypeRegistry,
    MenuTypeRef,
    menu_types_by_id,
    menu_types_by_key,
    allows_registering
);

crate::impl_registry!(
    MenuTypeRegistry,
    MenuType,
    menu_types_by_id,
    menu_types_by_key,
    menu_types
);
