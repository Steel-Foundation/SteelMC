//! This module contains everything related to text components.
use crate::{
    hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries},
    serial::ReadFrom,
    translations_registry::TRANSLATIONS,
};
use simdnbt::owned::read_tag;
use std::io::{self, Cursor};
use text_components::{
    TextComponent,
    content::{Content, NbtSource, Object, Resolvable},
    custom::CustomData,
    format::Format,
    interactivity::{ClickEvent, HoverEvent},
    resolving::TextResolutor,
};

/// A [`TextResolutor`] for the console
pub struct DisplayResolutor;
impl TextResolutor for DisplayResolutor {
    fn resolve_content(&self, resolvable: &Resolvable) -> TextComponent {
        TextComponent {
            content: Content::Resolvable(resolvable.clone()),
            ..Default::default()
        }
    }

    fn resolve_custom(&self, _data: &CustomData) -> Option<TextComponent> {
        None
    }

    fn translate(&self, key: &str) -> Option<String> {
        TRANSLATIONS.get(key).map(ToString::to_string)
    }
}

impl ReadFrom for TextComponent {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        use crate::codec::VarInt;

        // Minecraft's network format: VarInt length prefix, then NBT tag data
        let nbt_length = VarInt::read(data)?.0 as usize;

        if nbt_length == 0 {
            // Empty NBT means empty/default text component
            return Ok(Self::new());
        }

        // Read exactly one NBT tag using simdnbt
        let nbt_tag =
            read_tag(data).map_err(|e| io::Error::other(format!("Failed to read NBT: {e:?}")))?;

        Self::from_nbt(&nbt_tag)
            .ok_or_else(|| io::Error::other("Failed to parse TextComponent from NBT"))
    }
}

impl HashComponent for TextComponent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Minecraft's CODEC for Component uses an Either:
        // - If the component is plain text only (no siblings, no style), encode as just a string
        // - Otherwise, encode as a full map structure
        //
        // This matches ComponentSerialization.createCodec's tryCollapseToString logic
        if let Content::Text { text } = &self.content
            && self.format.is_none()
            && self.interactions.is_none()
            && self.children.is_empty()
        {
            hasher.put_string(text);
            return;
        }
        // Complex component - hash as a map structure
        hash_component_as_map(self, hasher);
    }
}

/// Hash this component as a map structure (for non-collapsible components).
fn hash_component_as_map(component: &TextComponent, hasher: &mut ComponentHasher) {
    // Collect all map entries with their key and value hashes for sorting
    let mut entries: Vec<HashEntry> = Vec::new();

    // Hash content
    hash_content_fields(&component.content, &mut entries);

    // Hash style fields
    hash_format_fields(&component.format, &mut entries);

    if let Some(insertion) = &component.interactions.insertion {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("insertion");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_string(insertion);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    if let Some(hover_event) = &component.interactions.hover {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("hover_event");
        let mut value_hasher = ComponentHasher::new();
        hash_hover_fields(hover_event, &mut value_hasher);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    if let Some(click_event) = &component.interactions.click {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("click_event");
        let mut value_hasher = ComponentHasher::new();
        hash_click_fields(click_event, &mut value_hasher);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // Hash extra (siblings)
    if !component.children.is_empty() {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("extra");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.start_list();
        for child in &component.children {
            // Each child is hashed as a complete component, then we write the 4-byte hash
            let child_hash = child.compute_hash();
            value_hasher.put_raw_bytes(&child_hash.to_le_bytes());
        }
        value_hasher.end_list();
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // Sort entries by key hash, then value hash (Minecraft's map ordering)
    sort_map_entries(&mut entries);

    // Write the sorted map
    // Important: Vanilla writes the 4-byte hash values, NOT the original encoded bytes!
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

#[allow(clippy::too_many_lines)]
fn hash_content_fields(content: &Content, entries: &mut Vec<HashEntry>) {
    match content {
        Content::Text { text } => {
            let mut key_hasher = ComponentHasher::new();
            key_hasher.put_string("text");
            let mut value_hasher = ComponentHasher::new();
            value_hasher.put_string(text);
            entries.push(HashEntry::new(key_hasher, value_hasher));
        }
        Content::Translate(message) => {
            // "translate" field
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("translate");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(&message.key);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            // "fallback" field (optional)
            if let Some(fallback) = &message.fallback {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("fallback");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(fallback);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            // "with" field (optional args list)
            if let Some(args) = &message.args
                && !args.is_empty()
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("with");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.start_list();
                for arg in args {
                    // Each argument is hashed as a complete component, then we write the 4-byte hash
                    let arg_hash = arg.compute_hash();
                    value_hasher.put_raw_bytes(&arg_hash.to_le_bytes());
                }
                value_hasher.end_list();
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        Content::Keybind { keybind } => {
            let mut key_hasher = ComponentHasher::new();
            key_hasher.put_string("keybind");
            let mut value_hasher = ComponentHasher::new();
            value_hasher.put_string(keybind);
            entries.push(HashEntry::new(key_hasher, value_hasher));
        }
        Content::Object(Object::Atlas { atlas, sprite }) => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("sprite");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(sprite);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            if let Some(atlas) = atlas {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("atlas");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(atlas);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        Content::Object(Object::Player { player, hat }) => {
            {
                let mut inner_entries: Vec<HashEntry> = Vec::new();
                if let Some(id) = &player.id {
                    let mut key_hasher = ComponentHasher::new();
                    key_hasher.put_string("id");
                    let mut value_hasher = ComponentHasher::new();
                    value_hasher.put_int_array(id);
                    inner_entries.push(HashEntry::new(key_hasher, value_hasher));
                }
                if let Some(name) = &player.name {
                    let mut key_hasher = ComponentHasher::new();
                    key_hasher.put_string("name");
                    let mut value_hasher = ComponentHasher::new();
                    value_hasher.put_string(name);
                    inner_entries.push(HashEntry::new(key_hasher, value_hasher));
                }
                if let Some(texture) = &player.texture {
                    let mut key_hasher = ComponentHasher::new();
                    key_hasher.put_string("texture");
                    let mut value_hasher = ComponentHasher::new();
                    value_hasher.put_string(texture);
                    inner_entries.push(HashEntry::new(key_hasher, value_hasher));
                }
                if !player.properties.is_empty() {
                    let mut key_hasher = ComponentHasher::new();
                    key_hasher.put_string("properties");
                    let mut value_hasher = ComponentHasher::new();
                    value_hasher.start_list();
                    for property in &player.properties {
                        let mut entries: Vec<HashEntry> = Vec::new();
                        {
                            let mut key_hasher = ComponentHasher::new();
                            key_hasher.put_string("name");
                            let mut value_hasher = ComponentHasher::new();
                            value_hasher.put_string(&property.name);
                            entries.push(HashEntry::new(key_hasher, value_hasher));
                        }
                        {
                            let mut key_hasher = ComponentHasher::new();
                            key_hasher.put_string("value");
                            let mut value_hasher = ComponentHasher::new();
                            value_hasher.put_string(&property.value);
                            entries.push(HashEntry::new(key_hasher, value_hasher));
                        }
                        if let Some(signature) = &property.signature {
                            let mut key_hasher = ComponentHasher::new();
                            key_hasher.put_string("signature");
                            let mut value_hasher = ComponentHasher::new();
                            value_hasher.put_string(signature);
                            entries.push(HashEntry::new(key_hasher, value_hasher));
                        }

                        // Sort entries by key hash, then value hash (Minecraft's map ordering)
                        sort_map_entries(&mut entries);
                        let mut hasher = ComponentHasher::new();
                        hasher.start_map();
                        for entry in entries {
                            hasher.put_raw_bytes(&entry.key_bytes);
                            hasher.put_raw_bytes(&entry.value_bytes);
                        }
                        hasher.end_map();
                        // List elements are hashes, not full encoded bytes
                        let property_hash = hasher.finish();
                        value_hasher.put_raw_bytes(&property_hash.to_le_bytes());
                    }
                    value_hasher.end_list();
                    inner_entries.push(HashEntry::new(key_hasher, value_hasher));
                }
                // Sort entries by key hash, then value hash (Minecraft's map ordering)
                sort_map_entries(&mut inner_entries);

                // Write the sorted map
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("player");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.start_map();
                for entry in inner_entries {
                    value_hasher.put_raw_bytes(&entry.key_bytes);
                    value_hasher.put_raw_bytes(&entry.value_bytes);
                }
                value_hasher.end_map();
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("hat");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_bool(*hat);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        Content::Resolvable(Resolvable::Entity {
            selector,
            separator,
        }) => {
            // "selector" field
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("selector");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(selector);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            // "separator" field
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("separator");
                let mut value_hasher = ComponentHasher::new();
                separator.hash_component(&mut value_hasher);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        Content::Resolvable(Resolvable::Scoreboard {
            selector,
            objective,
        }) => {
            // "score" object with "name" and "objective" fields
            let mut inner_entries: Vec<HashEntry> = Vec::new();
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("name");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(selector);
                inner_entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("objective");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(objective);
                inner_entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            // Sort entries by key hash, then value hash (Minecraft's map ordering)
            sort_map_entries(&mut inner_entries);

            // Write the sorted map under "score" key
            let mut key_hasher = ComponentHasher::new();
            key_hasher.put_string("score");
            let mut value_hasher = ComponentHasher::new();
            value_hasher.start_map();
            for entry in inner_entries {
                value_hasher.put_raw_bytes(&entry.key_bytes);
                value_hasher.put_raw_bytes(&entry.value_bytes);
            }
            value_hasher.end_map();
            entries.push(HashEntry::new(key_hasher, value_hasher));
        }
        Content::Resolvable(Resolvable::NBT {
            path,
            interpret,
            separator,
            source,
        }) => {
            // "nbt" field
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("nbt");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(path);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            // "interpret" field (optional)
            if let Some(interpret) = interpret {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("interpret");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_bool(*interpret);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            // "separator" field
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("separator");
                let mut value_hasher = ComponentHasher::new();
                separator.hash_component(&mut value_hasher);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            // Source field (entity, block, or storage)
            match source {
                NbtSource::Entity(selector) => {
                    let mut key_hasher = ComponentHasher::new();
                    key_hasher.put_string("entity");
                    let mut value_hasher = ComponentHasher::new();
                    value_hasher.put_string(selector);
                    entries.push(HashEntry::new(key_hasher, value_hasher));
                }
                NbtSource::Block(pos) => {
                    let mut key_hasher = ComponentHasher::new();
                    key_hasher.put_string("block");
                    let mut value_hasher = ComponentHasher::new();
                    value_hasher.put_string(pos);
                    entries.push(HashEntry::new(key_hasher, value_hasher));
                }
                NbtSource::Storage(id) => {
                    let mut key_hasher = ComponentHasher::new();
                    key_hasher.put_string("storage");
                    let mut value_hasher = ComponentHasher::new();
                    value_hasher.put_string(id);
                    entries.push(HashEntry::new(key_hasher, value_hasher));
                }
            }
        }
        Content::Custom(_custom_data) => {
            // Custom data components are resolved at runtime and should not appear
            // in hashing for network protocol. If they do appear, we treat them
            // as an empty text component.
            let mut key_hasher = ComponentHasher::new();
            key_hasher.put_string("text");
            let mut value_hasher = ComponentHasher::new();
            value_hasher.put_string("");
            entries.push(HashEntry::new(key_hasher, value_hasher));
        }
    }
}

/// Hash the style fields into the provided entries list for map hashing.
/// Field names match Minecraft's `Style.Serializer.MAP_CODEC`.
fn hash_format_fields(format: &Format, entries: &mut Vec<HashEntry>) {
    // color
    if let Some(color) = &format.color {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("color");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_string(&color.to_string());
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // shadow_color
    if let Some(shadow_color) = &format.shadow_color {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("shadow_color");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_long(*shadow_color);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // bold
    if let Some(bold) = format.bold {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("bold");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_bool(bold);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // italic
    if let Some(italic) = format.italic {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("italic");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_bool(italic);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // underlined
    if let Some(underlined) = format.underlined {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("underlined");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_bool(underlined);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // strikethrough
    if let Some(strikethrough) = format.strikethrough {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("strikethrough");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_bool(strikethrough);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // obfuscated
    if let Some(obfuscated) = format.obfuscated {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("obfuscated");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_bool(obfuscated);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }

    // font (encoded as a string identifier)
    if let Some(font) = &format.font {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("font");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_string(font);
        entries.push(HashEntry::new(key_hasher, value_hasher));
    }
}

fn hash_hover_fields(event: &HoverEvent, hasher: &mut ComponentHasher) {
    let mut entries: Vec<HashEntry> = Vec::new();

    match event {
        HoverEvent::ShowText { value } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("show_text");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("value");
                let mut value_hasher = ComponentHasher::new();
                hash_component_as_map(value, &mut value_hasher);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        HoverEvent::ShowItem {
            id,
            count,
            components,
        } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("show_item");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("id");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(id);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            if let Some(count) = count {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("count");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_int(*count);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            if let Some(components) = components {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("components");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(components);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        HoverEvent::ShowEntity { name, id, uuid } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("show_entity");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("id");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(id);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("uuid");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(&uuid.to_string());
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            if let Some(name) = name {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("name");
                let mut value_hasher = ComponentHasher::new();
                hash_component_as_map(name, &mut value_hasher);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
    }

    // Sort entries by key hash, then value hash (Minecraft's map ordering)
    sort_map_entries(&mut entries);

    // Write the sorted map
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

#[allow(clippy::too_many_lines)]
fn hash_click_fields(event: &ClickEvent, hasher: &mut ComponentHasher) {
    let mut entries: Vec<HashEntry> = Vec::new();

    match event {
        ClickEvent::OpenUrl { url } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("open_url");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("url");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(url);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        ClickEvent::RunCommand { command } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("run_command");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("command");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(command);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        ClickEvent::SuggestCommand { command } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("suggest_command");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("command");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(command);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        ClickEvent::ChangePage { page } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("change_page");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("page");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_int(*page);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        ClickEvent::CopyToClipboard { value } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("copy_to_clipboard");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("value");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(value);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        ClickEvent::ShowDialog { dialog } => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("show_dialog");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("dialog");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(dialog);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
        ClickEvent::Custom(custom_data) => {
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("action");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string("custom");
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
            {
                let mut key_hasher = ComponentHasher::new();
                key_hasher.put_string("id");
                let mut value_hasher = ComponentHasher::new();
                value_hasher.put_string(&custom_data.id);
                entries.push(HashEntry::new(key_hasher, value_hasher));
            }
        }
    }

    // Sort entries by key hash, then value hash (Minecraft's map ordering)
    sort_map_entries(&mut entries);

    // Write the sorted map
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}
