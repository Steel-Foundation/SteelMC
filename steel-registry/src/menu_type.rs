use crate::{RegistryEntry, RegistryExt, REGISTRY};
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

    pub fn register(&mut self, menu_type: MenuTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register menu types after the registry has been frozen"
        );

        let id = self.menu_types_by_id.len();
        self.menu_types_by_key.insert(menu_type.key.clone(), id);
        self.menu_types_by_id.push(menu_type);
        id
    }

    /// Replaces a menu_type at a given index.
    /// Returns true if the menu_type was replaced and false if the menu_type wasn't replaced
    #[must_use]
    pub fn replace(&mut self, menu_type: MenuTypeRef, id: usize) -> bool {
        if id >= self.menu_types_by_id.len() {
            return false;
        }
        self.menu_types_by_id[id] = menu_type;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<MenuTypeRef> {
        self.menu_types_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, menu_type: MenuTypeRef) -> &usize {
        self.menu_types_by_key
            .get(&menu_type.key)
            .expect("Menu type not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<MenuTypeRef> {
        self.menu_types_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, MenuTypeRef)> + '_ {
        self.menu_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &menu_type)| (id, menu_type))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.menu_types_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.menu_types_by_id.is_empty()
    }
}

impl RegistryExt for MenuTypeRegistry {
    type Entry = MenuTypeRef;

    fn freeze(&mut self) {
        self.allows_registering = false;
    }

    fn by_id(&self, id: usize) -> Option<MenuTypeRef> {
        self.menu_types_by_id.get(id).copied()
    }

    fn by_key(&self, key: &Identifier) -> Option<MenuTypeRef> {
        self.menu_types_by_key.get(key).and_then(|&id| self.by_id(id))
    }

    fn id_from_key(&self, key: &Identifier) -> Option<usize> {
        self.menu_types_by_key.get(key).copied()
    }

    fn len(&self) -> usize {
        self.menu_types_by_id.len()
    }

    fn is_empty(&self) -> bool {
        self.menu_types_by_id.is_empty()
    }
}

impl RegistryEntry for MenuType {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        REGISTRY.menu_types.id_from_key(&self.key)
    }
}

impl Default for MenuTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
