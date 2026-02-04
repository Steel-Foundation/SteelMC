use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a chat type definition from the data packs.
#[derive(Debug)]
pub struct ChatType {
    pub key: Identifier,
    pub chat: ChatTypeDecoration,
    pub narration: ChatTypeDecoration,
}

/// Defines the styling and translation for a part of a chat message.
#[derive(Debug)]
pub struct ChatTypeDecoration {
    pub translation_key: &'static str,
    pub parameters: &'static [&'static str],
    pub style: Option<ChatStyle>,
}

/// Defines optional text styling, like color and formatting.
#[derive(Debug)]
pub struct ChatStyle {
    pub color: Option<&'static str>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underlined: Option<bool>,
    pub strikethrough: Option<bool>,
    pub obfuscated: Option<bool>,
}

pub type ChatTypeRef = &'static ChatType;

pub struct ChatTypeRegistry {
    chat_types_by_id: Vec<ChatTypeRef>,
    chat_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ChatTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            chat_types_by_id: Vec::new(),
            chat_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, chat_type: ChatTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register chat types after the registry has been frozen"
        );

        let id = self.chat_types_by_id.len();
        self.chat_types_by_key.insert(chat_type.key.clone(), id);
        self.chat_types_by_id.push(chat_type);
        id
    }

    /// Replaces a chat_types at a given index.
    /// Returns true if the chat_types was replaced and false if the chat_types wasn't replaced
    #[must_use]
    pub fn replace(&mut self, chat_types: ChatTypeRef, id: usize) -> bool {
        if id >= self.chat_types_by_id.len() {
            return false;
        }
        self.chat_types_by_id[id] = chat_types;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<ChatTypeRef> {
        self.chat_types_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, chat_type: ChatTypeRef) -> &usize {
        self.chat_types_by_key
            .get(&chat_type.key)
            .expect("Chat type not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<ChatTypeRef> {
        self.chat_types_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ChatTypeRef)> + '_ {
        self.chat_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &ct)| (id, ct))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.chat_types_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chat_types_by_id.is_empty()
    }
}

impl RegistryExt for ChatTypeRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for ChatTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
