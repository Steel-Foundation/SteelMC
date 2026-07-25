use super::{
    ComponentData, ComponentPatchEntry, Cursor, DataComponentPatch, Identifier, ReadFrom, Result,
    VarInt, Write, WriteTo,
};

impl WriteTo for DataComponentPatch {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        use crate::{REGISTRY, RegistryExt};

        let mut added: Vec<(&Identifier, &ComponentData)> = Vec::new();
        let mut removed: Vec<&Identifier> = Vec::new();

        for (key, entry) in &self.entries {
            match entry {
                ComponentPatchEntry::Set(data) => added.push((key, data)),
                ComponentPatchEntry::Removed => removed.push(key),
            }
        }

        let added_count = i32::try_from(added.len())
            .map_err(|_| std::io::Error::other("Too many added data components"))?;
        let removed_count = i32::try_from(removed.len())
            .map_err(|_| std::io::Error::other("Too many removed data components"))?;
        VarInt(added_count).write(writer)?;
        VarInt(removed_count).write(writer)?;

        // Write added components
        for (key, data) in added {
            let id = REGISTRY
                .data_components
                .id_from_key(key)
                .ok_or_else(|| std::io::Error::other(format!("Unknown component key: {key:?}")))?;

            let entry = REGISTRY
                .data_components
                .by_id(id)
                .ok_or_else(|| std::io::Error::other(format!("No entry for component id: {id}")))?;

            VarInt(id as i32).write(writer)?;

            let mut buf = Vec::new();
            entry.write_network(data, &mut buf)?;
            writer.write_all(&buf)?;
        }

        // Write removed component IDs
        for key in removed {
            let id = REGISTRY
                .data_components
                .id_from_key(key)
                .ok_or_else(|| std::io::Error::other(format!("Unknown component key: {key:?}")))?;
            VarInt(id as i32).write(writer)?;
        }

        Ok(())
    }
}

impl ReadFrom for DataComponentPatch {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        use crate::{REGISTRY, RegistryExt};

        let added_count = read_component_count(data, "added")?;
        let removed_count = read_component_count(data, "removed")?;

        log::info!("Reading DataComponentPatch: added={added_count}, removed={removed_count}");

        let mut patch = Self::new();

        // Read added components
        for i in 0..added_count {
            let pos_before = data.position();
            let type_id = read_non_negative_varint(data, "component type id")?;

            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();

            log::info!("  [{i}] Reading component {key} (id={type_id}) at pos {pos_before}");

            let entry = REGISTRY
                .data_components
                .by_id(type_id)
                .ok_or_else(|| std::io::Error::other(format!("No entry for component: {key}")))?;

            let component_data = entry.read_network(data).map_err(|e| {
                log::error!("    Failed to read component {key}: {e}");
                e
            })?;

            let pos_after = data.position();
            log::info!("    Read {} bytes for {key}", pos_after - pos_before);

            patch
                .entries
                .insert(key, ComponentPatchEntry::Set(component_data));
        }

        // Read removed component IDs
        for _ in 0..removed_count {
            let type_id = read_non_negative_varint(data, "component type id")?;

            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();

            patch.entries.insert(key, ComponentPatchEntry::Removed);
        }

        Ok(patch)
    }
}

impl DataComponentPatch {
    /// Reads a patch where each component value is prefixed with a `VarInt` byte length.
    ///
    /// Vanilla uses this for untrusted client packets (e.g., creative mode slot)
    /// via `DataComponentPatch.DELIMITED_STREAM_CODEC`.
    pub fn read_delimited(data: &mut Cursor<&[u8]>) -> Result<Self> {
        use crate::{REGISTRY, RegistryExt};
        use std::io::Read;

        let added_count = read_component_count(data, "added")?;
        let removed_count = read_component_count(data, "removed")?;

        const MAX_COMPONENTS: usize = 65_536;
        const MAX_COMPONENT_BYTES: usize = 2 * 1024 * 1024;

        if added_count.saturating_add(removed_count) > MAX_COMPONENTS {
            return Err(std::io::Error::other(format!(
                "Component patch too large: {added_count} added + {removed_count} removed > {MAX_COMPONENTS}"
            )));
        }

        let mut patch = Self::new();

        for _ in 0..added_count {
            let type_id = read_non_negative_varint(data, "component type id")?;
            let byte_len = read_non_negative_varint(data, "component byte length")?;

            if byte_len > MAX_COMPONENT_BYTES {
                return Err(std::io::Error::other(format!(
                    "Component data too large: {byte_len} bytes > {MAX_COMPONENT_BYTES}"
                )));
            }

            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();

            let entry = REGISTRY
                .data_components
                .by_id(type_id)
                .ok_or_else(|| std::io::Error::other(format!("No entry for component: {key}")))?;

            // Read the component bytes into a sub-buffer
            let mut buf = vec![0u8; byte_len];
            data.read_exact(&mut buf)?;

            let mut sub_cursor = Cursor::new(buf.as_slice());
            let component_data = entry.read_network(&mut sub_cursor)?;
            patch
                .entries
                .insert(key, ComponentPatchEntry::Set(component_data));
        }

        for _ in 0..removed_count {
            let type_id = read_non_negative_varint(data, "component type id")?;
            let key = REGISTRY
                .data_components
                .get_key_by_id(type_id)
                .ok_or_else(|| {
                    std::io::Error::other(format!("Unknown component type ID: {type_id}"))
                })?
                .clone();
            patch.entries.insert(key, ComponentPatchEntry::Removed);
        }

        Ok(patch)
    }
}

fn read_component_count(data: &mut Cursor<&[u8]>, kind: &str) -> Result<usize> {
    read_non_negative_varint(data, &format!("{kind} component count"))
}

fn read_non_negative_varint(data: &mut Cursor<&[u8]>, name: &str) -> Result<usize> {
    let value = VarInt::read(data)?.0;
    usize::try_from(value).map_err(|_| std::io::Error::other(format!("Negative {name}: {value}")))
}
