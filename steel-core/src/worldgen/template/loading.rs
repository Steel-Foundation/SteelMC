use std::sync::{Arc, LazyLock};

use rustc_hash::FxHashMap;
use steel_utils::locks::SyncRwLock;

use super::*;

/// Process-wide cache of parsed vanilla structure templates.
///
/// Bundled template NBT is immutable, so each template is decompressed and
/// parsed at most once; every placement afterwards shares the parsed result.
static VANILLA_TEMPLATE_CACHE: LazyLock<SyncRwLock<FxHashMap<Identifier, Arc<StructureTemplate>>>> =
    LazyLock::new(|| SyncRwLock::new(FxHashMap::default()));

impl StructureTemplate {
    pub(crate) fn load_vanilla(registry: &Registry, key: &Identifier) -> Result<Arc<Self>, String> {
        if let Some(template) = VANILLA_TEMPLATE_CACHE.read().get(key) {
            return Ok(Arc::clone(template));
        }

        let mut cache = VANILLA_TEMPLATE_CACHE.write();
        if let Some(template) = cache.get(key) {
            return Ok(Arc::clone(template));
        }

        let bytes = vanilla_template_pools::vanilla_template_nbt_bytes(key)
            .ok_or_else(|| format!("vanilla structure template {key} is not bundled"))?;
        let template = Arc::new(Self::load_gzip_nbt(registry, bytes, &key.to_string())?);
        cache.insert(key.clone(), Arc::clone(&template));
        Ok(template)
    }

    pub(super) fn load_gzip_nbt(
        registry: &Registry,
        bytes: &[u8],
        context: &str,
    ) -> Result<Self, String> {
        let mut decoder = GzDecoder::new(bytes);
        let mut data = Vec::new();
        decoder
            .read_to_end(&mut data)
            .map_err(|err| format!("failed to decompress structure template {context}: {err}"))?;

        let nbt = read_nbt(&mut Cursor::new(&data))
            .map_err(|err| format!("failed to parse structure template {context}: {err}"))?;
        let root = match nbt {
            BorrowedNbt::Some(root) => root,
            BorrowedNbt::None => {
                return Err(format!("structure template {context} is empty"));
            }
        };
        let compound = root.as_compound();

        let size = Self::read_vec3(compound.list("size"), context, "size")?;
        let palettes = Self::read_palettes(registry, &compound, context)?;
        let blocks = compound
            .list("blocks")
            .and_then(|list| list.compounds())
            .ok_or_else(|| format!("structure template {context} has non-compound blocks list"))?;

        let mut loaded_palettes = Vec::with_capacity(palettes.len());
        for palette in &palettes {
            loaded_palettes.push(StructureTemplatePalette {
                blocks: Self::read_blocks(registry, &blocks, palette, context)?,
            });
        }

        let entities = Self::read_entities(registry, &compound, context)?;

        Ok(Self {
            size,
            palettes: loaded_palettes,
            entities,
        })
    }

    pub(super) fn read_vec3(
        list: Option<BorrowedNbtList<'_, '_>>,
        context: &str,
        field: &str,
    ) -> Result<IVec3, String> {
        let ints = list
            .and_then(|list| list.ints())
            .ok_or_else(|| format!("structure template {context} has non-int {field} list"))?;
        if ints.len() < 3 {
            return Err(format!(
                "structure template {context} {field} list has fewer than 3 entries"
            ));
        }
        Ok(IVec3::new(ints[0], ints[1], ints[2]))
    }

    pub(super) fn read_vec3d(
        list: Option<BorrowedNbtList<'_, '_>>,
        context: &str,
        field: &str,
    ) -> Result<DVec3, String> {
        let doubles = list
            .and_then(|list| list.doubles())
            .ok_or_else(|| format!("structure template {context} has non-double {field} list"))?;
        if doubles.len() < 3 {
            return Err(format!(
                "structure template {context} {field} list has fewer than 3 entries"
            ));
        }
        Ok(DVec3::new(doubles[0], doubles[1], doubles[2]))
    }

    pub(super) fn read_palettes(
        registry: &Registry,
        compound: &BorrowedNbtCompound<'_, '_>,
        context: &str,
    ) -> Result<Vec<Vec<BlockStateId>>, String> {
        if let Some(palette) = compound.list("palette").and_then(|list| list.compounds()) {
            return Ok(vec![Self::read_palette(registry, &palette, context)?]);
        }

        let palettes = compound
            .list("palettes")
            .and_then(|list| list.lists())
            .ok_or_else(|| {
                format!("structure template {context} is missing palette or palettes")
            })?;
        if palettes.is_empty() {
            return Err(format!(
                "structure template {context} has empty palettes list"
            ));
        }

        let mut result = Vec::with_capacity(palettes.len());
        for palette in palettes {
            let entries = palette.compounds().ok_or_else(|| {
                format!("structure template {context} has non-compound palette entry")
            })?;
            result.push(Self::read_palette(registry, &entries, context)?);
        }
        Ok(result)
    }

    pub(super) fn read_palette(
        registry: &Registry,
        entries: &BorrowedNbtCompoundList<'_, '_>,
        context: &str,
    ) -> Result<Vec<BlockStateId>, String> {
        let mut states = Vec::with_capacity(entries.len());
        for entry in entries.clone() {
            let Some(name) = entry.string("Name") else {
                return Err(format!(
                    "structure template {context} has palette entry without Name"
                ));
            };
            let name = Identifier::from_str(name.to_str().as_ref()).map_err(|err| {
                format!("structure template {context} has invalid block identifier: {err}")
            })?;
            let mut properties = BTreeMap::new();
            if let Some(props) = entry.compound("Properties") {
                for (key, value) in props.iter() {
                    let Some(value) = value.string() else {
                        return Err(format!(
                            "structure template {context} has non-string property {} on {name}",
                            key.to_str()
                        ));
                    };
                    properties.insert(key.to_str().into_owned(), value.to_str().into_owned());
                }
            }
            states.push(WorldgenStateResolver::block_state_from_data(
                registry,
                &BlockStateData { name, properties },
                "structure template palette",
            ));
        }
        Ok(states)
    }

    pub(super) fn read_blocks(
        registry: &Registry,
        blocks: &BorrowedNbtCompoundList<'_, '_>,
        palette: &[BlockStateId],
        context: &str,
    ) -> Result<Vec<StructureBlockInfo>, String> {
        let mut full_blocks = Vec::new();
        let mut other_blocks = Vec::new();
        let mut block_entities = Vec::new();

        for block in blocks.clone() {
            let pos = Self::read_vec3(block.list("pos"), context, "block pos")?;
            let state_index = block
                .int("state")
                .ok_or_else(|| format!("structure template {context} block is missing state"))?;
            if state_index < 0 {
                return Err(format!(
                    "structure template {context} has negative palette state {state_index}"
                ));
            }
            let state_index = usize::try_from(state_index).map_err(|_| {
                format!("structure template {context} state index does not fit usize")
            })?;
            let Some(&state) = palette.get(state_index) else {
                return Err(format!(
                    "structure template {context} state index {state_index} exceeds palette length {}",
                    palette.len()
                ));
            };
            let nbt = block.compound("nbt").map(|nbt| nbt.to_owned());
            let info = StructureBlockInfo {
                pos: BlockPos::new(pos[0], pos[1], pos[2]),
                state,
                nbt,
            };

            if info.nbt.is_some() {
                block_entities.push(info);
            } else if Self::is_static_full_block(registry, state) {
                full_blocks.push(info);
            } else {
                other_blocks.push(info);
            }
        }

        Self::sort_block_infos(&mut full_blocks);
        Self::sort_block_infos(&mut other_blocks);
        Self::sort_block_infos(&mut block_entities);

        full_blocks.extend(other_blocks);
        full_blocks.extend(block_entities);
        Ok(full_blocks)
    }

    pub(super) fn read_entities(
        registry: &Registry,
        compound: &BorrowedNbtCompound<'_, '_>,
        context: &str,
    ) -> Result<Vec<StructureEntityInfo>, String> {
        let Some(entities) = compound.list("entities").and_then(|list| list.compounds()) else {
            return Ok(Vec::new());
        };

        let mut result = Vec::with_capacity(entities.len());
        for entity in entities.clone() {
            let pos = Self::read_vec3d(entity.list("pos"), context, "entity pos")?;
            let block_pos = Self::read_vec3(entity.list("blockPos"), context, "entity blockPos")?;
            let entity_nbt = entity.compound("nbt").ok_or_else(|| {
                format!("structure template {context} has entity entry without nbt")
            })?;
            let id = entity_nbt
                .string("id")
                .ok_or_else(|| format!("structure template {context} has entity nbt without id"))?;
            let id = Identifier::from_str(id.to_str().as_ref()).map_err(|err| {
                format!("structure template {context} has invalid entity identifier: {err}")
            })?;
            let entity_type = registry.entity_types.by_key(&id).ok_or_else(|| {
                format!("structure template {context} references unknown entity type {id}")
            })?;
            let rotation = Self::read_entity_rotation(&entity_nbt);
            let velocity = Self::read_optional_vec3d(&entity_nbt, "Motion");
            let fall_distance = entity_nbt.double("fall_distance").unwrap_or(0.0);
            let fire_freeze = EntityFireFreezeState::from_parts(
                Self::read_optional_int(&entity_nbt, "Fire").unwrap_or(0),
                Self::read_optional_int(&entity_nbt, "TicksFrozen").unwrap_or(0),
                false,
                false,
                entity_nbt
                    .byte("HasVisualFire")
                    .is_some_and(|value| value != 0),
            );
            let on_ground = entity_nbt.byte("OnGround").is_some_and(|value| value != 0);
            let save_data = EntityBaseSaveData {
                air_supply: Self::read_optional_int(&entity_nbt, "Air")
                    .unwrap_or(DEFAULT_MAX_AIR_SUPPLY),
                portal_cooldown: Self::read_optional_int(&entity_nbt, "PortalCooldown")
                    .unwrap_or(0),
                no_gravity: entity_nbt.byte("NoGravity").is_some_and(|value| value != 0),
                invulnerable: entity_nbt
                    .byte("Invulnerable")
                    .is_some_and(|value| value != 0),
                custom_name: Self::read_custom_name(&entity_nbt),
                custom_name_visible: entity_nbt
                    .byte("CustomNameVisible")
                    .is_some_and(|value| value != 0),
                silent: entity_nbt.byte("Silent").is_some_and(|value| value != 0),
                glowing: entity_nbt.byte("Glowing").is_some_and(|value| value != 0),
                tags: Self::read_entity_tags(&entity_nbt),
                custom_data: entity_nbt
                    .compound("data")
                    .map_or_else(NbtCompound::new, |compound| compound.to_owned()),
            };
            let mut nbt = entity_nbt.to_owned();
            Self::strip_entity_base_fields(&mut nbt);

            result.push(StructureEntityInfo {
                pos,
                block_pos: BlockPos::new(block_pos[0], block_pos[1], block_pos[2]),
                entity_type,
                rotation,
                velocity,
                fall_distance,
                fire_freeze,
                on_ground,
                save_data,
                nbt,
            });
        }

        Ok(result)
    }

    pub(super) fn read_entity_rotation(nbt: &BorrowedNbtCompound<'_, '_>) -> (f32, f32) {
        let Some(rotation) = nbt.list("Rotation").and_then(|list| list.floats()) else {
            return (0.0, 0.0);
        };
        if rotation.len() < 2 {
            return (0.0, 0.0);
        }
        (rotation[0], rotation[1])
    }

    pub(super) fn read_optional_vec3d(nbt: &BorrowedNbtCompound<'_, '_>, field: &str) -> DVec3 {
        let Some(values) = nbt.list(field).and_then(|list| list.doubles()) else {
            return DVec3::ZERO;
        };
        if values.len() < 3 {
            return DVec3::ZERO;
        }
        DVec3::new(values[0], values[1], values[2])
    }

    pub(super) fn read_optional_int(nbt: &BorrowedNbtCompound<'_, '_>, field: &str) -> Option<i32> {
        nbt.int(field)
            .or_else(|| nbt.short(field).map(i32::from))
            .or_else(|| nbt.byte(field).map(i32::from))
    }

    pub(super) fn read_custom_name(nbt: &BorrowedNbtCompound<'_, '_>) -> Option<TextComponent> {
        let tag = nbt.get("CustomName")?;
        TextComponent::from_nbt(&tag.to_owned())
    }

    pub(super) fn read_entity_tags(nbt: &BorrowedNbtCompound<'_, '_>) -> BTreeSet<String> {
        nbt.list("Tags")
            .and_then(|list| list.strings())
            .map(|tags| {
                tags.iter()
                    .take(MAX_ENTITY_TAGS)
                    .map(|tag| tag.to_str().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn strip_entity_base_fields(nbt: &mut NbtCompound) {
        for field in [
            "id",
            "Pos",
            "Motion",
            "Rotation",
            "UUID",
            "fall_distance",
            "Fire",
            "Air",
            "OnGround",
            "NoGravity",
            "Invulnerable",
            "PortalCooldown",
            "CustomName",
            "CustomNameVisible",
            "Silent",
            "Glowing",
            "TicksFrozen",
            "HasVisualFire",
            "Tags",
            "data",
        ] {
            let _ = nbt.remove(field);
        }
    }

    pub(super) fn is_static_full_block(registry: &Registry, state: BlockStateId) -> bool {
        let Some(block) = registry.blocks.by_state_id(state) else {
            return false;
        };
        !block.config.dynamic_shape
            && blocks::shapes::is_shape_full_block(
                registry.blocks.get_static_collision_shape(state),
            )
    }

    pub(super) fn sort_block_infos(blocks: &mut [StructureBlockInfo]) {
        blocks.sort_by(|left, right| {
            left.pos
                .y()
                .cmp(&right.pos.y())
                .then(left.pos.x().cmp(&right.pos.x()))
                .then(left.pos.z().cmp(&right.pos.z()))
        });
    }

    pub(crate) const fn size(&self, rotation: Rotation) -> IVec3 {
        rotation.rotate_size(self.size)
    }

    pub(crate) const fn zero_position_with_transform(
        &self,
        zero_pos: BlockPos,
        rotation: Rotation,
    ) -> BlockPos {
        let x = self.size.x - 1;
        let z = self.size.z - 1;
        match rotation {
            Rotation::None => zero_pos,
            Rotation::Clockwise90 => zero_pos.offset(z, 0, 0),
            Rotation::Clockwise180 => zero_pos.offset(x, 0, z),
            Rotation::CounterClockwise90 => zero_pos.offset(0, 0, x),
        }
    }
}
