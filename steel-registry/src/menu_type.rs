use steel_utils::Identifier;

/// Represents a menu type (container/GUI type) in Minecraft.
/// Menu types define the different inventory interfaces available,
/// such as chests, furnaces, anvils, etc.
#[derive(Debug)]
pub struct MenuType {
    pub key: Identifier,
}

crate::define_registry!(MenuTypeRegistry, MenuType, stem: menu_types);
