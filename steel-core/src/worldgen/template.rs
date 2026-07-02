use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::str::FromStr;

use flate2::read::GzDecoder;
use glam::{DVec3, IVec3};
use simdnbt::borrow::{
    Nbt as BorrowedNbt, NbtCompound as BorrowedNbtCompound,
    NbtCompoundList as BorrowedNbtCompoundList, NbtList as BorrowedNbtList, read as read_nbt,
    read_compound as read_borrowed_compound,
};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::properties::Direction as BlockPropertyDirection;
use steel_registry::blocks::properties::{BlockStateProperties, Half};
use steel_registry::blocks::{self};
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidState;
use steel_registry::shared_structs::BlockStateData;
use steel_registry::structure::LiquidSettingsData;
use steel_registry::structure_processor::{
    PosRuleTestData, ProcessorRuleData, RuleBlockEntityModifierData, StructureProcessorAxis,
    StructureProcessorKind, StructureRuleTestData,
};
use steel_registry::template_pool::Projection;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{
    Registry, RegistryExt, TaggedRegistryExt, vanilla_block_entity_types, vanilla_blocks,
    vanilla_template_pools,
};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::random::{PositionalRandom, Random, RandomSource};
use steel_utils::value_providers::IntProvider;
use steel_utils::{
    BlockPos, BlockStateId, BoundingBox, Direction, Identifier, Rotation, types::UpdateFlags,
};
use text_components::TextComponent;
use uuid::Uuid;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::chunk::heightmap::HeightmapType;
use crate::entity::{
    DEFAULT_MAX_AIR_SUPPLY, ENTITIES, EntityBaseSaveData, EntityFireFreezeState, EntityLoadRequest,
    MAX_ENTITY_TAGS,
};
use crate::worldgen::region::WorldGenRegion;
use steel_worldgen::state_resolver::WorldgenStateResolver;
use steel_worldgen::structure::{StructureBlockIgnore, StructureMirror};

/// Loaded vanilla structure template payload.
///
/// Steel keeps template data separate from template-pool metadata. Pools only need jigsaw
/// summaries during structure-start planning; feature and piece placement need the full NBT
/// block payload and processors, so this type mirrors vanilla's loaded `StructureTemplate`.
#[derive(Debug, Clone)]
pub(crate) struct StructureTemplate {
    size: IVec3,
    palettes: Vec<StructureTemplatePalette>,
    entities: Vec<StructureEntityInfo>,
}

#[derive(Debug, Clone)]
struct StructureTemplatePalette {
    blocks: Vec<StructureBlockInfo>,
}

#[derive(Debug, Clone)]
struct StructureBlockInfo {
    pos: BlockPos,
    state: BlockStateId,
    nbt: Option<NbtCompound>,
}

#[derive(Debug, Clone)]
struct StructureEntityInfo {
    pos: DVec3,
    block_pos: BlockPos,
    entity_type: EntityTypeRef,
    rotation: (f32, f32),
    velocity: DVec3,
    fall_distance: f64,
    fire_freeze: EntityFireFreezeState,
    on_ground: bool,
    save_data: EntityBaseSaveData,
    nbt: NbtCompound,
}

#[derive(Debug, Clone, PartialEq)]
struct ProcessedBlockInfo {
    template_pos: BlockPos,
    world_pos: BlockPos,
    state: BlockStateId,
    nbt: Option<NbtCompound>,
}

pub(crate) struct StructureDataMarker {
    pub(crate) metadata: String,
    pub(crate) pos: BlockPos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructureProcessorRandom {
    /// Vanilla `StructurePlaceSettings.setRandom(random)`.
    Placement,
    /// Vanilla `StructurePlaceSettings.getRandom(pos)` fallback.
    Positional,
}

pub(crate) struct StructurePlaceSettings<'a> {
    pub(crate) mirror: StructureMirror,
    pub(crate) rotation: Rotation,
    pub(crate) rotation_pivot: BlockPos,
    pub(crate) bounding_box: BoundingBox,
    pub(crate) processors: &'a [StructureProcessorKind],
    pub(crate) block_ignore: StructureBlockIgnore,
    pub(crate) late_block_ignore: StructureBlockIgnore,
    pub(crate) replace_jigsaws: bool,
    pub(crate) projection: Option<Projection>,
    pub(crate) processor_random: StructureProcessorRandom,
    pub(crate) liquid_settings: LiquidSettingsData,
}

impl StructureTemplate {
    pub(crate) fn load_vanilla(registry: &Registry, key: &Identifier) -> Result<Self, String> {
        let Some(bytes) = vanilla_template_pools::vanilla_template_nbt_bytes(key) else {
            return Err(format!("vanilla structure template {key} is not bundled"));
        };
        Self::load_gzip_nbt(registry, bytes, &key.to_string())
    }

    fn load_gzip_nbt(registry: &Registry, bytes: &[u8], context: &str) -> Result<Self, String> {
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

    fn read_vec3(
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

    fn read_vec3d(
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

    fn read_palettes(
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

    fn read_palette(
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

    fn read_blocks(
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

    fn read_entities(
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

    fn read_entity_rotation(nbt: &BorrowedNbtCompound<'_, '_>) -> (f32, f32) {
        let Some(rotation) = nbt.list("Rotation").and_then(|list| list.floats()) else {
            return (0.0, 0.0);
        };
        if rotation.len() < 2 {
            return (0.0, 0.0);
        }
        (rotation[0], rotation[1])
    }

    fn read_optional_vec3d(nbt: &BorrowedNbtCompound<'_, '_>, field: &str) -> DVec3 {
        let Some(values) = nbt.list(field).and_then(|list| list.doubles()) else {
            return DVec3::ZERO;
        };
        if values.len() < 3 {
            return DVec3::ZERO;
        }
        DVec3::new(values[0], values[1], values[2])
    }

    fn read_optional_int(nbt: &BorrowedNbtCompound<'_, '_>, field: &str) -> Option<i32> {
        nbt.int(field)
            .or_else(|| nbt.short(field).map(i32::from))
            .or_else(|| nbt.byte(field).map(i32::from))
    }

    fn read_custom_name(nbt: &BorrowedNbtCompound<'_, '_>) -> Option<TextComponent> {
        let tag = nbt.get("CustomName")?;
        TextComponent::from_nbt(&tag.to_owned())
    }

    fn read_entity_tags(nbt: &BorrowedNbtCompound<'_, '_>) -> BTreeSet<String> {
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

    fn strip_entity_base_fields(nbt: &mut NbtCompound) {
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

    fn is_static_full_block(registry: &Registry, state: BlockStateId) -> bool {
        let Some(block) = registry.blocks.by_state_id(state) else {
            return false;
        };
        !block.config.dynamic_shape
            && blocks::shapes::is_shape_full_block(
                registry.blocks.get_static_collision_shape(state),
            )
    }

    fn sort_block_infos(blocks: &mut [StructureBlockInfo]) {
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

    pub(crate) fn bounding_box(&self, pos: BlockPos, rotation: Rotation) -> BoundingBox {
        rotation.get_bounding_box(pos.0, self.size)
    }

    pub(crate) fn bounding_box_with_transform(
        &self,
        position: BlockPos,
        rotation: Rotation,
        mirror: StructureMirror,
        pivot: BlockPos,
    ) -> BoundingBox {
        let corner1 = Self::calculate_relative_position(BlockPos::ZERO, mirror, rotation, pivot);
        let corner2 =
            Self::calculate_relative_position(BlockPos(self.size - 1), mirror, rotation, pivot);
        BoundingBox::new(position.0 + corner1.0, position.0 + corner2.0)
    }

    pub(crate) const fn calculate_relative_position(
        pos: BlockPos,
        mirror: StructureMirror,
        rotation: Rotation,
        pivot: BlockPos,
    ) -> BlockPos {
        let (x, z) = match mirror {
            StructureMirror::None => (pos.x(), pos.z()),
            StructureMirror::FrontBack => (-pos.x(), pos.z()),
            StructureMirror::LeftRight => (pos.x(), -pos.z()),
        };
        let pos = rotation.transform_pos(IVec3::new(x, pos.y(), z), pivot.0);
        BlockPos(pos)
    }

    fn transform_entity_position(
        pos: DVec3,
        mirror: StructureMirror,
        rotation: Rotation,
        pivot: BlockPos,
    ) -> DVec3 {
        let mut x = pos.x;
        let y = pos.y;
        let mut z = pos.z;
        match mirror {
            StructureMirror::LeftRight => z = 1.0 - z,
            StructureMirror::FrontBack => x = 1.0 - x,
            StructureMirror::None => {}
        }

        let pivot_x = f64::from(pivot.x());
        let pivot_z = f64::from(pivot.z());
        match rotation {
            Rotation::CounterClockwise90 => {
                DVec3::new(pivot_x - pivot_z + z, y, pivot_x + pivot_z + 1.0 - x)
            }
            Rotation::Clockwise90 => {
                DVec3::new(pivot_x + pivot_z + 1.0 - z, y, pivot_z - pivot_x + x)
            }
            Rotation::Clockwise180 => {
                DVec3::new(pivot_x + pivot_x + 1.0 - x, y, pivot_z + pivot_z + 1.0 - z)
            }
            Rotation::None => DVec3::new(x, y, z),
        }
    }

    fn transform_entity_rotation(
        (yaw, pitch): (f32, f32),
        mirror: StructureMirror,
        rotation: Rotation,
    ) -> (f32, f32) {
        let yaw = Self::wrap_degrees(yaw);
        let rotated = match rotation {
            Rotation::Clockwise180 => yaw + 180.0,
            Rotation::CounterClockwise90 => yaw + 270.0,
            Rotation::Clockwise90 => yaw + 90.0,
            Rotation::None => yaw,
        };
        let mirrored = match mirror {
            StructureMirror::FrontBack => -yaw,
            StructureMirror::LeftRight => 180.0 - yaw,
            StructureMirror::None => yaw,
        };
        (rotated + mirrored - yaw, pitch)
    }

    fn transform_entity_additional_nbt(
        nbt: &mut NbtCompound,
        mirror: StructureMirror,
        rotation: Rotation,
    ) {
        let Some(facing) = Self::entity_facing(nbt) else {
            return;
        };
        let facing = Self::mirror_direction(rotation.rotate(facing), mirror);
        let _ = nbt.remove("Facing");
        nbt.insert("Facing", Self::entity_facing_value(facing));
    }

    fn entity_facing(nbt: &NbtCompound) -> Option<Direction> {
        nbt.byte("Facing")
            .map(i32::from)
            .or_else(|| nbt.int("Facing"))
            .and_then(Self::direction_from_entity_facing)
    }

    const fn direction_from_entity_facing(value: i32) -> Option<Direction> {
        match value {
            0 => Some(Direction::Down),
            1 => Some(Direction::Up),
            2 => Some(Direction::North),
            3 => Some(Direction::South),
            4 => Some(Direction::West),
            5 => Some(Direction::East),
            _ => None,
        }
    }

    const fn entity_facing_value(direction: Direction) -> i8 {
        match direction {
            Direction::Down => 0,
            Direction::Up => 1,
            Direction::North => 2,
            Direction::South => 3,
            Direction::West => 4,
            Direction::East => 5,
        }
    }

    fn wrap_degrees(mut degrees: f32) -> f32 {
        degrees %= 360.0;
        if degrees >= 180.0 {
            degrees -= 360.0;
        }
        if degrees < -180.0 {
            degrees += 360.0;
        }
        degrees
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "structure placement call mirrors vanilla template placement context"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "template placement follows vanilla's single-pass block placement flow"
    )]
    pub(crate) fn place_in_world(
        &self,
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        reference_pos: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
        flags: UpdateFlags,
    ) -> bool {
        let Some(palette) = self.palette(settings, position, random) else {
            return false;
        };
        if (palette.blocks.is_empty() && self.entities.is_empty())
            || [self.size.x, self.size.y, self.size.z]
                .iter()
                .any(|&axis| axis < 1)
        {
            return false;
        }
        let mut original_blocks = Vec::with_capacity(palette.blocks.len());
        let mut processed_blocks = Vec::with_capacity(palette.blocks.len());

        for block in &palette.blocks {
            let original = ProcessedBlockInfo {
                template_pos: block.pos,
                world_pos: block.pos,
                state: block.state,
                nbt: block.nbt.clone(),
            };
            let processed = ProcessedBlockInfo {
                template_pos: block.pos,
                world_pos: Self::transformed_position(position, block.pos, settings),
                state: block.state,
                nbt: block.nbt.clone(),
            };

            let Some(processed) = Self::process_block(
                region,
                registry,
                &original,
                processed,
                settings,
                reference_pos,
                random,
            ) else {
                continue;
            };

            original_blocks.push(original);
            processed_blocks.push(processed);
        }

        let processed_blocks = Self::finalize_processing(
            region,
            registry,
            position,
            reference_pos,
            settings,
            &original_blocks,
            processed_blocks,
            random,
        );

        let mut placed_any = false;
        let mut placed_positions = Vec::with_capacity(processed_blocks.len());
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut min_z = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let mut max_z = i32::MIN;
        let mut to_fill = Vec::new();
        let mut locked_fluids = Vec::new();
        let apply_waterlogging = settings.liquid_settings == LiquidSettingsData::ApplyWaterlogging;
        for processed in processed_blocks {
            if !settings.bounding_box.contains_blockpos(processed.world_pos) {
                continue;
            }

            let final_state = Self::transform_state(
                registry,
                processed.state,
                settings.mirror,
                settings.rotation,
            );
            let previous_fluid_state =
                apply_waterlogging.then(|| Self::fluid_state_at(region, processed.world_pos));
            if processed.nbt.is_some() {
                let barrier_flags = UpdateFlags::UPDATE_INVISIBLE
                    | UpdateFlags::UPDATE_KNOWN_SHAPE
                    | UpdateFlags::UPDATE_SUPPRESS_DROPS
                    | UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS
                    | UpdateFlags::UPDATE_SKIP_ON_PLACE;
                let _ = region.set_block_state(
                    processed.world_pos,
                    vanilla_blocks::BARRIER.default_state(),
                    barrier_flags,
                );
            }

            if !region.set_block_state(processed.world_pos, final_state, flags) {
                continue;
            }
            placed_any = true;
            min_x = min_x.min(processed.world_pos.x());
            min_y = min_y.min(processed.world_pos.y());
            min_z = min_z.min(processed.world_pos.z());
            max_x = max_x.max(processed.world_pos.x());
            max_y = max_y.max(processed.world_pos.y());
            max_z = max_z.max(processed.world_pos.z());
            placed_positions.push(processed.world_pos);

            if let Some(mut nbt) = processed.nbt {
                let block_entity_type =
                    Self::block_entity_type_for_nbt_or_state(registry, final_state, &nbt);
                if Self::should_reseed_template_loot(block_entity_type, &nbt) {
                    nbt.insert("LootTableSeed", NbtTag::Long(random.next_i64()));
                }
                Self::place_block_entity(
                    region,
                    processed.world_pos,
                    final_state,
                    block_entity_type,
                    nbt,
                );
            } else {
                let _ = region.remove_block_entity(processed.world_pos);
            }

            if let Some(previous_fluid_state) = previous_fluid_state {
                if Self::fluid_state_for_block(final_state).is_source() {
                    locked_fluids.push(processed.world_pos);
                } else if Self::is_liquid_block_container(final_state) {
                    let _ = Self::place_liquid(
                        region,
                        processed.world_pos,
                        final_state,
                        previous_fluid_state,
                    );
                    if !previous_fluid_state.is_source() {
                        to_fill.push(processed.world_pos);
                    }
                }
            }
        }

        Self::fill_neighbor_source_liquids(region, &mut to_fill, &locked_fluids);

        if placed_any && !flags.contains(UpdateFlags::UPDATE_KNOWN_SHAPE) {
            Self::update_shape_at_edge(
                region,
                flags,
                &placed_positions,
                BlockPos::new(min_x, min_y, min_z),
                BlockPos::new(max_x, max_y, max_z),
            );

            let placed_update_flags =
                (flags & !UpdateFlags::UPDATE_NEIGHBORS) | UpdateFlags::UPDATE_KNOWN_SHAPE;
            for pos in placed_positions {
                let state = region.block_state(pos);
                let new_state = Self::update_from_neighbor_shapes(region, state, pos);
                if state != new_state {
                    let _ = region.set_block_state(pos, new_state, placed_update_flags);
                }
            }
        }

        self.place_entities(region, position, settings);

        true
    }

    fn place_entities(
        &self,
        region: &mut WorldGenRegion<'_>,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
    ) {
        if self.entities.is_empty() {
            return;
        }

        let world_offset = DVec3::new(
            f64::from(position.x()),
            f64::from(position.y()),
            f64::from(position.z()),
        );
        for entity in &self.entities {
            let block_pos = Self::calculate_relative_position(
                entity.block_pos,
                settings.mirror,
                settings.rotation,
                settings.rotation_pivot,
            )
            .offset(position.x(), position.y(), position.z());
            if !settings.bounding_box.contains_blockpos(block_pos) {
                continue;
            }

            let pos = Self::transform_entity_position(
                entity.pos,
                settings.mirror,
                settings.rotation,
                settings.rotation_pivot,
            ) + world_offset;
            let rotation = Self::transform_entity_rotation(
                entity.rotation,
                settings.mirror,
                settings.rotation,
            );
            let mut nbt = entity.nbt.clone();
            Self::transform_entity_additional_nbt(&mut nbt, settings.mirror, settings.rotation);

            let mut nbt_bytes = Vec::new();
            nbt.write(&mut nbt_bytes);
            let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(&nbt_bytes)) else {
                log::warn!(
                    "failed to reborrow owned NBT for structure template entity {}",
                    entity.entity_type.key
                );
                continue;
            };

            let runtime_entity = ENTITIES.create_and_load_or_raw(
                EntityLoadRequest {
                    entity_type: entity.entity_type,
                    position: pos,
                    uuid: Uuid::new_v4(),
                    velocity: entity.velocity,
                    rotation,
                    fall_distance: entity.fall_distance,
                    fire_freeze: entity.fire_freeze,
                    on_ground: entity.on_ground,
                    save_data: entity.save_data.clone(),
                    world: region.weak_world(),
                },
                &nbt,
            );
            let _ = region.add_fresh_entity(runtime_entity);
        }
    }

    pub(crate) fn replace_jigsaw_final_states(
        &self,
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) {
        let Some(palette) = self.palette(settings, position, random) else {
            return;
        };

        for block in &palette.blocks {
            if Self::block_for_state(registry, block.state) != &vanilla_blocks::JIGSAW {
                continue;
            }
            let world_pos = Self::transformed_position(position, block.pos, settings);
            if !settings.bounding_box.contains_blockpos(world_pos) {
                continue;
            }
            let Some(nbt) = block.nbt.as_ref() else {
                continue;
            };
            let final_state = nbt
                .string("final_state")
                .map_or_else(|| "minecraft:air".into(), |value| value.to_str());
            let state = Self::parse_block_state_string(registry, final_state.as_ref())
                .unwrap_or_else(|| vanilla_blocks::AIR.default_state());
            let _ = region.set_block_state(world_pos, state, UpdateFlags::UPDATE_ALL);
        }
    }

    pub(crate) fn data_markers(
        &self,
        registry: &Registry,
        position: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) -> Vec<StructureDataMarker> {
        let Some(palette) = self.palette(settings, position, random) else {
            return Vec::new();
        };

        let mut markers = Vec::new();
        for block in &palette.blocks {
            if Self::block_for_state(registry, block.state) != &vanilla_blocks::STRUCTURE_BLOCK {
                continue;
            }
            let world_pos = Self::transformed_position(position, block.pos, settings);
            if !settings.bounding_box.contains_blockpos(world_pos) {
                continue;
            }
            let Some(nbt) = block.nbt.as_ref() else {
                continue;
            };
            if nbt
                .string("mode")
                .is_none_or(|mode| mode.to_str().as_ref() != "DATA")
            {
                continue;
            }
            let metadata = nbt
                .string("metadata")
                .map(|metadata| metadata.to_str().into_owned())
                .unwrap_or_default();
            markers.push(StructureDataMarker {
                metadata,
                pos: world_pos,
            });
        }
        markers
    }

    fn update_shape_at_edge(
        region: &WorldGenRegion<'_>,
        flags: UpdateFlags,
        placed_positions: &[BlockPos],
        min: BlockPos,
        max: BlockPos,
    ) {
        let filled = placed_positions
            .iter()
            .map(|pos| (pos.x() - min.x(), pos.y() - min.y(), pos.z() - min.z()))
            .collect::<BTreeSet<_>>();
        let x_size = max.x() - min.x() + 1;
        let y_size = max.y() - min.y() + 1;
        let z_size = max.z() - min.z() + 1;
        let edge_flags = flags & !UpdateFlags::UPDATE_NEIGHBORS;

        Self::for_all_shape_faces(
            x_size,
            y_size,
            z_size,
            |x, y, z| filled.contains(&(x, y, z)),
            |direction, x, y, z| {
                let pos = min.offset(x, y, z);
                let neighbor_pos = pos.relative(direction);
                let state = region.block_state(pos);
                let neighbor_state = region.block_state(neighbor_pos);
                let new_state = BLOCK_BEHAVIORS
                    .get_behavior(state.get_block())
                    .update_shape(state, region, pos, direction, neighbor_pos, neighbor_state);
                if state != new_state {
                    let _ = region.set_block_state(pos, new_state, edge_flags);
                }

                let new_neighbor_state = BLOCK_BEHAVIORS
                    .get_behavior(neighbor_state.get_block())
                    .update_shape(
                        neighbor_state,
                        region,
                        neighbor_pos,
                        direction.opposite(),
                        pos,
                        new_state,
                    );
                if neighbor_state != new_neighbor_state {
                    let _ = region.set_block_state(neighbor_pos, new_neighbor_state, edge_flags);
                }
            },
        );
    }

    fn update_from_neighbor_shapes(
        region: &WorldGenRegion<'_>,
        state: BlockStateId,
        pos: BlockPos,
    ) -> BlockStateId {
        let mut updated = state;
        for direction in Direction::UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = region.block_state(neighbor_pos);
            updated = BLOCK_BEHAVIORS
                .get_behavior(updated.get_block())
                .update_shape(
                    updated,
                    region,
                    pos,
                    direction,
                    neighbor_pos,
                    neighbor_state,
                );
        }
        updated
    }

    fn fill_neighbor_source_liquids(
        region: &WorldGenRegion<'_>,
        to_fill: &mut Vec<BlockPos>,
        locked_fluids: &[BlockPos],
    ) {
        const DIRECTIONS: [Direction; 5] = [
            Direction::Up,
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ];

        let mut filled = true;
        while filled && !to_fill.is_empty() {
            filled = false;
            let mut index = 0;
            while index < to_fill.len() {
                let pos = to_fill[index];
                let mut to_place = Self::fluid_state_at(region, pos);
                for direction in DIRECTIONS {
                    if to_place.is_source() {
                        break;
                    }
                    let neighbor_pos = pos.relative(direction);
                    let neighbor = Self::fluid_state_at(region, neighbor_pos);
                    if neighbor.is_source() && !locked_fluids.contains(&neighbor_pos) {
                        to_place = neighbor;
                    }
                }

                if to_place.is_source() {
                    let state = region.block_state(pos);
                    if Self::is_liquid_block_container(state) {
                        let _ = Self::place_liquid(region, pos, state, to_place);
                        filled = true;
                        to_fill.remove(index);
                        continue;
                    }
                }

                index += 1;
            }
        }
    }

    fn fluid_state_at(region: &WorldGenRegion<'_>, pos: BlockPos) -> FluidState {
        Self::fluid_state_for_block(region.block_state(pos))
    }

    fn fluid_state_for_block(state: BlockStateId) -> FluidState {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .get_fluid_state(state)
    }

    fn is_liquid_block_container(state: BlockStateId) -> bool {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .is_liquid_container(state)
    }

    fn place_liquid(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        behavior.place_liquid(region, pos, state, fluid_state)
    }

    fn for_all_shape_faces(
        x_size: i32,
        y_size: i32,
        z_size: i32,
        is_full: impl Fn(i32, i32, i32) -> bool,
        mut consumer: impl FnMut(Direction, i32, i32, i32),
    ) {
        for x in 0..x_size {
            for y in 0..y_size {
                let mut last_full = false;
                for z in 0..=z_size {
                    let full = z != z_size && is_full(x, y, z);
                    if !last_full && full {
                        consumer(Direction::North, x, y, z);
                    }
                    if last_full && !full {
                        consumer(Direction::South, x, y, z - 1);
                    }
                    last_full = full;
                }
            }
        }

        for z in 0..z_size {
            for x in 0..x_size {
                let mut last_full = false;
                for y in 0..=y_size {
                    let full = y != y_size && is_full(x, y, z);
                    if !last_full && full {
                        consumer(Direction::Down, x, y, z);
                    }
                    if last_full && !full {
                        consumer(Direction::Up, x, y - 1, z);
                    }
                    last_full = full;
                }
            }
        }

        for y in 0..y_size {
            for z in 0..z_size {
                let mut last_full = false;
                for x in 0..=x_size {
                    let full = x != x_size && is_full(x, y, z);
                    if !last_full && full {
                        consumer(Direction::West, x, y, z);
                    }
                    if last_full && !full {
                        consumer(Direction::East, x - 1, y, z);
                    }
                    last_full = full;
                }
            }
        }
    }

    fn palette(
        &self,
        settings: &StructurePlaceSettings<'_>,
        position: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<&StructureTemplatePalette> {
        if self.palettes.is_empty() {
            return None;
        }
        let Ok(bound) = i32::try_from(self.palettes.len()) else {
            panic!(
                "structure template palette count {} exceeds i32 range",
                self.palettes.len()
            );
        };
        let index = match settings.processor_random {
            StructureProcessorRandom::Placement => random.next_i32_bounded(bound),
            StructureProcessorRandom::Positional => {
                let mut random = LegacyRandom::from_seed(Self::block_pos_seed(position) as u64);
                random.next_i32_bounded(bound)
            }
        };
        Some(&self.palettes[index as usize])
    }

    const fn transformed_position(
        position: BlockPos,
        template_pos: BlockPos,
        settings: &StructurePlaceSettings<'_>,
    ) -> BlockPos {
        let transformed = Self::calculate_relative_position(
            template_pos,
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        );
        position.offset(transformed.x(), transformed.y(), transformed.z())
    }

    fn process_block(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        original: &ProcessedBlockInfo,
        initial: ProcessedBlockInfo,
        settings: &StructurePlaceSettings<'_>,
        reference_pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<ProcessedBlockInfo> {
        let mut current = initial;
        if settings.block_ignore.ignores(registry, current.state) {
            return None;
        }

        if settings.replace_jigsaws {
            current = Self::replace_jigsaw_block(registry, current)?;
        }

        for processor in settings.processors {
            current = Self::process_block_with_processor(
                region,
                registry,
                processor,
                original,
                current,
                settings,
                reference_pos,
                random,
            )?;
        }
        if settings.projection == Some(Projection::TerrainMatching) {
            current = Self::apply_terrain_matching_projection(region, original, current);
        }
        if settings.late_block_ignore.ignores(registry, current.state) {
            return None;
        }
        Some(current)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "processor calls mirror vanilla StructureProcessor.processBlock inputs"
    )]
    fn process_block_with_processor(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        processor: &StructureProcessorKind,
        original: &ProcessedBlockInfo,
        current: ProcessedBlockInfo,
        settings: &StructurePlaceSettings<'_>,
        reference_pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<ProcessedBlockInfo> {
        match processor {
            StructureProcessorKind::BlockRot {
                rottable_blocks,
                integrity,
            } => {
                if rottable_blocks.as_ref().is_some_and(|tag| {
                    !registry
                        .blocks
                        .is_in_tag(Self::block_for_state(registry, original.state), tag)
                }) {
                    return Some(current);
                }
                (Self::processor_next_f32(settings, current.world_pos, random) <= *integrity)
                    .then_some(current)
            }
            StructureProcessorKind::ProtectedBlocks { cannot_replace } => {
                let existing =
                    Self::block_for_state(registry, region.block_state(current.world_pos));
                (!existing.has_tag(cannot_replace)).then_some(current)
            }
            StructureProcessorKind::Rule { rules } => {
                let mut rule_random =
                    LegacyRandom::from_seed(Self::block_pos_seed(current.world_pos) as u64);
                let location_state = region.block_state(current.world_pos);
                for rule in rules {
                    if Self::rule_matches(
                        registry,
                        rule,
                        current.state,
                        location_state,
                        original.template_pos,
                        current.world_pos,
                        reference_pos,
                        &mut rule_random,
                    ) {
                        return Some(Self::apply_rule(registry, rule, current, &mut rule_random));
                    }
                }
                Some(current)
            }
            StructureProcessorKind::BlockAge { mossiness } => Some(Self::process_block_age(
                registry, current, *mossiness, settings, random,
            )),
            StructureProcessorKind::LavaSubmergedBlock => Some(Self::process_lava_submerged_block(
                registry,
                region.block_state(current.world_pos),
                current,
            )),
            StructureProcessorKind::BlackstoneReplace => {
                Some(Self::process_blackstone_replace(registry, current))
            }
            StructureProcessorKind::Capped { .. } => Some(current),
        }
    }

    fn process_block_age(
        registry: &Registry,
        current: ProcessedBlockInfo,
        mossiness: f32,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) -> ProcessedBlockInfo {
        match settings.processor_random {
            StructureProcessorRandom::Placement => {
                Self::process_block_age_with_random(registry, current, mossiness, random)
            }
            StructureProcessorRandom::Positional => {
                let mut random =
                    LegacyRandom::from_seed(Self::block_pos_seed(current.world_pos) as u64);
                Self::process_block_age_with_random(registry, current, mossiness, &mut random)
            }
        }
    }

    fn process_block_age_with_random(
        registry: &Registry,
        mut current: ProcessedBlockInfo,
        mossiness: f32,
        random: &mut impl Random,
    ) -> ProcessedBlockInfo {
        let block = Self::block_for_state(registry, current.state);
        let new_state = if block == &vanilla_blocks::STONE_BRICKS
            || block == &vanilla_blocks::STONE
            || block == &vanilla_blocks::CHISELED_STONE_BRICKS
        {
            Self::maybe_replace_full_stone_block(registry, mossiness, random)
        } else if block.has_tag(&BlockTag::STAIRS) {
            Self::maybe_replace_stairs(registry, current.state, mossiness, random)
        } else if block.has_tag(&BlockTag::SLABS) {
            Self::maybe_replace_slab(registry, current.state, mossiness, random)
        } else if block.has_tag(&BlockTag::WALLS) {
            Self::maybe_replace_wall(registry, current.state, mossiness, random)
        } else if block == &vanilla_blocks::OBSIDIAN {
            Self::maybe_replace_obsidian(registry, random)
        } else {
            None
        };

        if let Some(new_state) = new_state {
            current.state = new_state;
        }
        current
    }

    fn maybe_replace_full_stone_block(
        registry: &Registry,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        if random.next_f32() >= 0.5 {
            return None;
        }

        let non_mossy = [
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::CRACKED_STONE_BRICKS),
            Self::random_facing_stairs(registry, &vanilla_blocks::STONE_BRICK_STAIRS, random),
        ];
        let mossy = [
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::MOSSY_STONE_BRICKS),
            Self::random_facing_stairs(registry, &vanilla_blocks::MOSSY_STONE_BRICK_STAIRS, random),
        ];
        let candidates = if random.next_f32() < mossiness {
            mossy
        } else {
            non_mossy
        };
        Some(candidates[random.next_i32_bounded(2) as usize])
    }

    fn maybe_replace_stairs(
        registry: &Registry,
        state: BlockStateId,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        if random.next_f32() >= 0.5 {
            return None;
        }

        let non_mossy = [
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::STONE_SLAB),
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::STONE_BRICK_SLAB),
        ];
        let mossy = [
            registry
                .blocks
                .copy_matching_properties(state, &vanilla_blocks::MOSSY_STONE_BRICK_STAIRS),
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::MOSSY_STONE_BRICK_SLAB),
        ];
        let candidates = if random.next_f32() < mossiness {
            mossy
        } else {
            non_mossy
        };
        Some(candidates[random.next_i32_bounded(2) as usize])
    }

    fn maybe_replace_slab(
        registry: &Registry,
        state: BlockStateId,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        (random.next_f32() < mossiness).then(|| {
            registry
                .blocks
                .copy_matching_properties(state, &vanilla_blocks::MOSSY_STONE_BRICK_SLAB)
        })
    }

    fn maybe_replace_wall(
        registry: &Registry,
        state: BlockStateId,
        mossiness: f32,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        (random.next_f32() < mossiness).then(|| {
            registry
                .blocks
                .copy_matching_properties(state, &vanilla_blocks::MOSSY_STONE_BRICK_WALL)
        })
    }

    fn maybe_replace_obsidian(
        registry: &Registry,
        random: &mut impl Random,
    ) -> Option<BlockStateId> {
        (random.next_f32() < 0.15).then(|| {
            registry
                .blocks
                .get_default_state_id(&vanilla_blocks::CRYING_OBSIDIAN)
        })
    }

    fn random_facing_stairs(
        registry: &Registry,
        block: BlockRef,
        random: &mut impl Random,
    ) -> BlockStateId {
        const HORIZONTAL_DIRECTIONS: [BlockPropertyDirection; 4] = [
            BlockPropertyDirection::North,
            BlockPropertyDirection::East,
            BlockPropertyDirection::South,
            BlockPropertyDirection::West,
        ];

        let facing = HORIZONTAL_DIRECTIONS[random.next_i32_bounded(4) as usize];
        let half = if random.next_i32_bounded(2) == 0 {
            Half::Top
        } else {
            Half::Bottom
        };
        let state = registry.blocks.get_default_state_id(block);
        let state = registry
            .blocks
            .set_property(state, &BlockStateProperties::FACING, facing);
        registry
            .blocks
            .set_property(state, &BlockStateProperties::HALF, half)
    }

    fn process_lava_submerged_block(
        registry: &Registry,
        existing_state: BlockStateId,
        mut current: ProcessedBlockInfo,
    ) -> ProcessedBlockInfo {
        if Self::block_for_state(registry, existing_state) == &vanilla_blocks::LAVA
            && !blocks::shapes::is_offset_shape_full_block(
                registry
                    .blocks
                    .get_outline_shape_at(current.state, current.world_pos),
            )
        {
            current.state = registry.blocks.get_default_state_id(&vanilla_blocks::LAVA);
        }
        current
    }

    fn process_blackstone_replace(
        registry: &Registry,
        mut current: ProcessedBlockInfo,
    ) -> ProcessedBlockInfo {
        let Some(block) =
            Self::blackstone_replacement_block(Self::block_for_state(registry, current.state))
        else {
            return current;
        };

        let mut new_state = registry.blocks.get_default_state_id(block);
        if let Some(facing) = registry
            .blocks
            .try_get_property(current.state, &BlockStateProperties::FACING)
            && registry
                .blocks
                .try_get_property(new_state, &BlockStateProperties::FACING)
                .is_some()
        {
            new_state =
                registry
                    .blocks
                    .set_property(new_state, &BlockStateProperties::FACING, facing);
        }
        if let Some(half) = registry
            .blocks
            .try_get_property(current.state, &BlockStateProperties::HALF)
            && registry
                .blocks
                .try_get_property(new_state, &BlockStateProperties::HALF)
                .is_some()
        {
            new_state = registry
                .blocks
                .set_property(new_state, &BlockStateProperties::HALF, half);
        }
        if let Some(slab_type) = registry
            .blocks
            .try_get_property(current.state, &BlockStateProperties::SLAB_TYPE)
            && registry
                .blocks
                .try_get_property(new_state, &BlockStateProperties::SLAB_TYPE)
                .is_some()
        {
            new_state = registry.blocks.set_property(
                new_state,
                &BlockStateProperties::SLAB_TYPE,
                slab_type,
            );
        }

        current.state = new_state;
        current
    }

    fn blackstone_replacement_block(block: BlockRef) -> Option<BlockRef> {
        if block == &vanilla_blocks::COBBLESTONE || block == &vanilla_blocks::MOSSY_COBBLESTONE {
            Some(&vanilla_blocks::BLACKSTONE)
        } else if block == &vanilla_blocks::STONE {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE)
        } else if block == &vanilla_blocks::STONE_BRICKS
            || block == &vanilla_blocks::MOSSY_STONE_BRICKS
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICKS)
        } else if block == &vanilla_blocks::COBBLESTONE_STAIRS
            || block == &vanilla_blocks::MOSSY_COBBLESTONE_STAIRS
        {
            Some(&vanilla_blocks::BLACKSTONE_STAIRS)
        } else if block == &vanilla_blocks::STONE_STAIRS {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_STAIRS)
        } else if block == &vanilla_blocks::STONE_BRICK_STAIRS
            || block == &vanilla_blocks::MOSSY_STONE_BRICK_STAIRS
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICK_STAIRS)
        } else if block == &vanilla_blocks::COBBLESTONE_SLAB
            || block == &vanilla_blocks::MOSSY_COBBLESTONE_SLAB
        {
            Some(&vanilla_blocks::BLACKSTONE_SLAB)
        } else if block == &vanilla_blocks::SMOOTH_STONE_SLAB
            || block == &vanilla_blocks::STONE_SLAB
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_SLAB)
        } else if block == &vanilla_blocks::STONE_BRICK_SLAB
            || block == &vanilla_blocks::MOSSY_STONE_BRICK_SLAB
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICK_SLAB)
        } else if block == &vanilla_blocks::STONE_BRICK_WALL
            || block == &vanilla_blocks::MOSSY_STONE_BRICK_WALL
        {
            Some(&vanilla_blocks::POLISHED_BLACKSTONE_BRICK_WALL)
        } else if block == &vanilla_blocks::COBBLESTONE_WALL
            || block == &vanilla_blocks::MOSSY_COBBLESTONE_WALL
        {
            Some(&vanilla_blocks::BLACKSTONE_WALL)
        } else if block == &vanilla_blocks::CHISELED_STONE_BRICKS {
            Some(&vanilla_blocks::CHISELED_POLISHED_BLACKSTONE)
        } else if block == &vanilla_blocks::CRACKED_STONE_BRICKS {
            Some(&vanilla_blocks::CRACKED_POLISHED_BLACKSTONE_BRICKS)
        } else if block == &vanilla_blocks::IRON_BARS {
            Some(&vanilla_blocks::IRON_CHAIN)
        } else {
            None
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "processor finalization receives vanilla's full template processing context"
    )]
    fn finalize_processing(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        reference_pos: BlockPos,
        settings: &StructurePlaceSettings<'_>,
        original_blocks: &[ProcessedBlockInfo],
        mut processed_blocks: Vec<ProcessedBlockInfo>,
        random: &mut WorldgenRandom,
    ) -> Vec<ProcessedBlockInfo> {
        for processor in settings.processors {
            if let StructureProcessorKind::Capped { delegate, limit } = processor {
                processed_blocks = Self::finalize_capped_processing(
                    region,
                    registry,
                    position,
                    reference_pos,
                    delegate,
                    limit,
                    original_blocks,
                    processed_blocks,
                    settings,
                    random,
                );
            }
        }
        processed_blocks
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches vanilla CappedProcessor.finalizeProcessing inputs"
    )]
    fn finalize_capped_processing(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        position: BlockPos,
        reference_pos: BlockPos,
        delegate: &StructureProcessorKind,
        limit: &IntProvider,
        original_blocks: &[ProcessedBlockInfo],
        mut processed_blocks: Vec<ProcessedBlockInfo>,
        settings: &StructurePlaceSettings<'_>,
        random: &mut WorldgenRandom,
    ) -> Vec<ProcessedBlockInfo> {
        if limit.max() == 0 || processed_blocks.is_empty() {
            return processed_blocks;
        }
        if original_blocks.len() != processed_blocks.len() {
            return processed_blocks;
        }

        let Ok(processed_len_i32) = i32::try_from(processed_blocks.len()) else {
            panic!(
                "processed structure block list length {} exceeds i32 range",
                processed_blocks.len()
            );
        };

        let mut cap_random = Self::capped_processor_random(region.seed(), position);
        let max_to_replace = limit.sample(&mut cap_random).min(processed_len_i32);
        if max_to_replace < 1 {
            return processed_blocks;
        }

        let mut indices = (0..processed_blocks.len()).collect::<Vec<_>>();
        Self::vanilla_shuffle(&mut indices, &mut cap_random);

        let mut replaced = 0;
        for index in indices {
            if replaced >= max_to_replace {
                break;
            }

            let current = processed_blocks[index].clone();
            let Some(altered) = Self::process_block_with_processor(
                region,
                registry,
                delegate,
                &original_blocks[index],
                current,
                settings,
                reference_pos,
                random,
            ) else {
                continue;
            };

            if altered != processed_blocks[index] {
                processed_blocks[index] = altered;
                replaced += 1;
            }
        }

        processed_blocks
    }

    fn processor_next_f32(
        settings: &StructurePlaceSettings<'_>,
        pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> f32 {
        match settings.processor_random {
            StructureProcessorRandom::Placement => random.next_f32(),
            StructureProcessorRandom::Positional => {
                let mut random = LegacyRandom::from_seed(Self::block_pos_seed(pos) as u64);
                random.next_f32()
            }
        }
    }

    fn capped_processor_random(world_seed: i64, position: BlockPos) -> RandomSource {
        LegacyRandom::from_seed(world_seed as u64)
            .next_positional()
            .at(position.x(), position.y(), position.z())
    }

    fn vanilla_shuffle<T>(items: &mut [T], random: &mut impl Random) {
        for i in (1..items.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!(
                    "structure processor shuffle length {} exceeds i32 range",
                    items.len()
                );
            };
            let j = random.next_i32_bounded(bound) as usize;
            items.swap(i, j);
        }
    }

    fn replace_jigsaw_block(
        registry: &Registry,
        mut current: ProcessedBlockInfo,
    ) -> Option<ProcessedBlockInfo> {
        if Self::block_for_state(registry, current.state) != &vanilla_blocks::JIGSAW {
            return Some(current);
        }

        let Some(nbt) = current.nbt.as_ref() else {
            return Some(current);
        };
        let final_state = nbt
            .string("final_state")
            .map_or_else(|| "minecraft:air".into(), |value| value.to_str());
        current.state = Self::parse_block_state_string(registry, final_state.as_ref())
            .unwrap_or_else(|| vanilla_blocks::AIR.default_state());
        current.nbt = None;

        (Self::block_for_state(registry, current.state) != &vanilla_blocks::STRUCTURE_VOID)
            .then_some(current)
    }

    fn parse_block_state_string(registry: &Registry, value: &str) -> Option<BlockStateId> {
        let (name, rest) = Self::read_block_identifier_prefix(value)?;
        let id = Identifier::from_str(name).ok()?;
        let block = registry.blocks.by_key(&id)?;

        let mut parsed_properties = Vec::new();
        if rest.starts_with('[') {
            let properties = Self::read_block_state_properties_prefix(rest)?;
            if !properties.is_empty() {
                for property in properties.split(',') {
                    let (key, value) = property.split_once('=')?;
                    parsed_properties.push((key, value));
                }
            }
        }

        registry
            .blocks
            .state_id_from_block_defaulted_properties(block, parsed_properties)
    }

    fn read_block_identifier_prefix(value: &str) -> Option<(&str, &str)> {
        let end = value
            .char_indices()
            .find_map(|(index, char)| {
                (char != ':' && !Identifier::valid_char(char)).then_some(index)
            })
            .unwrap_or(value.len());
        (end > 0).then_some((&value[..end], &value[end..]))
    }

    fn read_block_state_properties_prefix(rest: &str) -> Option<&str> {
        let rest = rest.strip_prefix('[')?;
        let end = rest.find(']')?;
        Some(&rest[..end])
    }

    fn apply_terrain_matching_projection(
        region: &WorldGenRegion<'_>,
        original: &ProcessedBlockInfo,
        mut current: ProcessedBlockInfo,
    ) -> ProcessedBlockInfo {
        let height = region.height_at(
            HeightmapType::WorldSurfaceWg,
            current.world_pos.x(),
            current.world_pos.z(),
        ) - 1;
        current.world_pos = BlockPos::new(
            current.world_pos.x(),
            height + original.template_pos.y(),
            current.world_pos.z(),
        );
        current
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "processor rules receive the same state and position tuple as vanilla"
    )]
    fn rule_matches(
        registry: &Registry,
        rule: &ProcessorRuleData,
        input_state: BlockStateId,
        location_state: BlockStateId,
        template_pos: BlockPos,
        world_pos: BlockPos,
        reference_pos: BlockPos,
        random: &mut LegacyRandom,
    ) -> bool {
        Self::rule_test_matches(registry, &rule.input_predicate, input_state, random)
            && Self::rule_test_matches(registry, &rule.location_predicate, location_state, random)
            && Self::pos_rule_test_matches(
                &rule.position_predicate,
                template_pos,
                world_pos,
                reference_pos,
                random,
            )
    }

    fn rule_test_matches(
        registry: &Registry,
        test: &StructureRuleTestData,
        state: BlockStateId,
        random: &mut LegacyRandom,
    ) -> bool {
        match test {
            StructureRuleTestData::AlwaysTrue => true,
            StructureRuleTestData::BlockMatch { block } => registry
                .blocks
                .by_key(block)
                .is_some_and(|block_ref| Self::block_for_state(registry, state) == block_ref),
            StructureRuleTestData::RandomBlockMatch { block, probability } => {
                registry
                    .blocks
                    .by_key(block)
                    .is_some_and(|block_ref| Self::block_for_state(registry, state) == block_ref)
                    && random.next_f32() < *probability
            }
            StructureRuleTestData::TagMatch { tag } => registry
                .blocks
                .is_in_tag(Self::block_for_state(registry, state), tag),
            StructureRuleTestData::BlockStateMatch { block_state } => {
                state
                    == WorldgenStateResolver::block_state_from_data(
                        registry,
                        block_state,
                        "structure processor block-state predicate",
                    )
            }
        }
    }

    fn pos_rule_test_matches(
        test: &PosRuleTestData,
        _template_pos: BlockPos,
        world_pos: BlockPos,
        reference_pos: BlockPos,
        random: &mut LegacyRandom,
    ) -> bool {
        match test {
            PosRuleTestData::AlwaysTrue => true,
            PosRuleTestData::AxisAlignedLinearPos {
                axis,
                min_chance,
                max_chance,
                min_dist,
                max_dist,
            } => {
                let dist = match axis {
                    StructureProcessorAxis::X => (world_pos.x() - reference_pos.x()).abs(),
                    StructureProcessorAxis::Y => (world_pos.y() - reference_pos.y()).abs(),
                    StructureProcessorAxis::Z => (world_pos.z() - reference_pos.z()).abs(),
                };
                random.next_f32()
                    <= Self::clamped_lerp_inverse(
                        dist,
                        *min_dist,
                        *max_dist,
                        *min_chance,
                        *max_chance,
                    )
            }
        }
    }

    fn apply_rule(
        registry: &Registry,
        rule: &ProcessorRuleData,
        mut current: ProcessedBlockInfo,
        random: &mut LegacyRandom,
    ) -> ProcessedBlockInfo {
        current.state = WorldgenStateResolver::block_state_from_data(
            registry,
            &rule.output_state,
            "structure processor output state",
        );
        current.nbt = match &rule.block_entity_modifier {
            RuleBlockEntityModifierData::Passthrough => current.nbt,
            RuleBlockEntityModifierData::AppendLoot { loot_table } => {
                let mut nbt = current.nbt.unwrap_or_default();
                nbt.insert("LootTable", NbtTag::String(loot_table.to_string().into()));
                nbt.insert("LootTableSeed", NbtTag::Long(random.next_i64()));
                Some(nbt)
            }
        };
        current
    }

    fn place_block_entity(
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        block_entity_type: Option<BlockEntityTypeRef>,
        nbt: NbtCompound,
    ) {
        let Some(block_entity_type) = block_entity_type else {
            return;
        };
        let _ = region.set_block_entity_data(pos, block_entity_type, state, nbt);
    }

    fn block_entity_type_for_nbt_or_state(
        registry: &Registry,
        state: BlockStateId,
        nbt: &NbtCompound,
    ) -> Option<BlockEntityTypeRef> {
        if let Some(id) = nbt.string("id") {
            let id = Identifier::from_str(id.to_str().as_ref()).ok()?;
            return registry.block_entity_types.by_key(&id);
        }
        Self::block_entity_type_for_state(registry, state)
    }

    fn block_entity_type_for_state(
        registry: &Registry,
        state: BlockStateId,
    ) -> Option<BlockEntityTypeRef> {
        let block = Self::block_for_state(registry, state);
        if block == &vanilla_blocks::SUSPICIOUS_SAND || block == &vanilla_blocks::SUSPICIOUS_GRAVEL
        {
            return Some(&vanilla_block_entity_types::BRUSHABLE_BLOCK);
        }
        None
    }

    fn should_reseed_template_loot(
        block_entity_type: Option<BlockEntityTypeRef>,
        nbt: &NbtCompound,
    ) -> bool {
        nbt.contains("LootTable")
            && block_entity_type.is_some_and(Self::is_randomizable_container_block_entity)
    }

    fn is_randomizable_container_block_entity(block_entity_type: BlockEntityTypeRef) -> bool {
        let key = &block_entity_type.key;
        key == &vanilla_block_entity_types::BARREL.key
            || key == &vanilla_block_entity_types::CHEST.key
            || key == &vanilla_block_entity_types::TRAPPED_CHEST.key
            || key == &vanilla_block_entity_types::DISPENSER.key
            || key == &vanilla_block_entity_types::DROPPER.key
            || key == &vanilla_block_entity_types::HOPPER.key
            || key == &vanilla_block_entity_types::SHULKER_BOX.key
            || key == &vanilla_block_entity_types::CRAFTER.key
    }

    pub(crate) fn transform_state(
        registry: &Registry,
        state: BlockStateId,
        mirror: StructureMirror,
        rotation: Rotation,
    ) -> BlockStateId {
        if mirror == StructureMirror::None && rotation == Rotation::None {
            return state;
        }

        let Some(block) = registry.blocks.by_state_id(state) else {
            return state;
        };
        let mut properties = registry
            .blocks
            .get_properties(state)
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect::<Vec<_>>();

        Self::mirror_string_properties(&mut properties, mirror);
        Self::rotate_string_properties(&mut properties, rotation);
        let property_refs = properties
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let Some(rotated) = registry
            .blocks
            .state_id_from_properties(&block.key, &property_refs)
        else {
            panic!(
                "rotating block state {} produced invalid properties",
                block.key
            );
        };
        rotated
    }

    fn block_for_state(registry: &Registry, state: BlockStateId) -> BlockRef {
        let Some(block) = registry.blocks.by_state_id(state) else {
            panic!(
                "structure template references invalid block state {}",
                state.0
            );
        };
        block
    }

    fn rotate_string_properties(properties: &mut [(String, String)], rotation: Rotation) {
        let original = properties.to_vec();
        for (name, value) in properties.iter_mut() {
            match name.as_str() {
                "axis"
                    if matches!(
                        rotation,
                        Rotation::Clockwise90 | Rotation::CounterClockwise90
                    ) =>
                {
                    match value.as_str() {
                        "x" => "z".clone_into(value),
                        "z" => "x".clone_into(value),
                        _ => {}
                    }
                }
                "facing" => {
                    if let Some(direction) = Self::parse_direction(value) {
                        rotation.rotate(direction).as_str().clone_into(value);
                    }
                }
                "rotation" => {
                    if let Ok(segment) = value.parse::<i32>() {
                        let rotated = match rotation {
                            Rotation::None => segment,
                            Rotation::Clockwise90 => segment + 4,
                            Rotation::Clockwise180 => segment + 8,
                            Rotation::CounterClockwise90 => segment + 12,
                        };
                        *value = (rotated & 15).to_string();
                    }
                }
                "shape" => {
                    if let Some(rotated) = Self::rotate_rail_shape(value, rotation) {
                        rotated.clone_into(value);
                    }
                }
                "north" | "east" | "south" | "west" => {
                    let from = Self::direction_from_property_name(name);
                    let source = Self::inverse_rotate_direction(rotation, from);
                    if let Some(source_name) = Self::property_name_from_direction(source)
                        && let Some((_, source_value)) = original
                            .iter()
                            .find(|(original_name, _)| original_name == source_name)
                    {
                        value.clone_from(source_value);
                    }
                }
                _ => {}
            }
        }
    }

    fn mirror_string_properties(properties: &mut [(String, String)], mirror: StructureMirror) {
        if mirror == StructureMirror::None {
            return;
        }

        let original = properties.to_vec();
        let facing = original
            .iter()
            .find(|(name, _)| name == "facing")
            .and_then(|(_, value)| Self::parse_direction(value));
        let stair_shape = original
            .iter()
            .find(|(name, _)| name == "shape")
            .and_then(|(_, value)| Self::parse_stair_shape(value));

        let mirrored_stairs = facing
            .zip(stair_shape)
            .and_then(|(direction, shape)| Self::mirror_stair_shape(direction, shape, mirror));

        for (name, value) in properties.iter_mut() {
            match name.as_str() {
                "facing" => {
                    if let Some((mirrored_facing, _)) = mirrored_stairs {
                        mirrored_facing.as_str().clone_into(value);
                    } else if let Some(direction) = Self::parse_direction(value) {
                        Self::mirror_direction(direction, mirror)
                            .as_str()
                            .clone_into(value);
                    }
                }
                "rotation" => {
                    if let Ok(segment) = value.parse::<i32>() {
                        *value = Self::mirror_rotation_segment(segment, 16, mirror).to_string();
                    }
                }
                "hinge" => match value.as_str() {
                    "left" => "right".clone_into(value),
                    "right" => "left".clone_into(value),
                    _ => {}
                },
                "shape" => {
                    if let Some((_, mirrored_shape)) = mirrored_stairs {
                        mirrored_shape.clone_into(value);
                    } else if let Some(mirrored_shape) = Self::mirror_rail_shape(value, mirror) {
                        mirrored_shape.clone_into(value);
                    }
                }
                "north" | "east" | "south" | "west" => {
                    let from = Self::direction_from_property_name(name);
                    let source = Self::mirror_direction(from, mirror);
                    if let Some(source_name) = Self::property_name_from_direction(source)
                        && let Some((_, source_value)) = original
                            .iter()
                            .find(|(original_name, _)| original_name == source_name)
                    {
                        value.clone_from(source_value);
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_direction(value: &str) -> Option<Direction> {
        match value {
            "down" => Some(BlockPropertyDirection::Down),
            "up" => Some(BlockPropertyDirection::Up),
            "north" => Some(BlockPropertyDirection::North),
            "south" => Some(BlockPropertyDirection::South),
            "west" => Some(BlockPropertyDirection::West),
            "east" => Some(BlockPropertyDirection::East),
            _ => None,
        }
    }

    fn direction_from_property_name(name: &str) -> Direction {
        match name {
            "east" => BlockPropertyDirection::East,
            "south" => BlockPropertyDirection::South,
            "west" => BlockPropertyDirection::West,
            _ => BlockPropertyDirection::North,
        }
    }

    const fn mirror_direction(direction: Direction, mirror: StructureMirror) -> Direction {
        match mirror {
            StructureMirror::FrontBack => match direction {
                BlockPropertyDirection::West => BlockPropertyDirection::East,
                BlockPropertyDirection::East => BlockPropertyDirection::West,
                other => other,
            },
            StructureMirror::LeftRight => match direction {
                BlockPropertyDirection::North => BlockPropertyDirection::South,
                BlockPropertyDirection::South => BlockPropertyDirection::North,
                other => other,
            },
            StructureMirror::None => direction,
        }
    }

    const fn mirror_rotation_segment(rotation: i32, steps: i32, mirror: StructureMirror) -> i32 {
        let half_steps = steps / 2;
        let corrected = if rotation > half_steps {
            rotation - steps
        } else {
            rotation
        };
        match mirror {
            StructureMirror::LeftRight => (half_steps - corrected + steps) % steps,
            StructureMirror::FrontBack => (steps - corrected) % steps,
            StructureMirror::None => rotation,
        }
    }

    const fn inverse_rotate_direction(rotation: Rotation, direction: Direction) -> Direction {
        match rotation {
            Rotation::None => direction,
            Rotation::Clockwise90 => Rotation::CounterClockwise90.rotate(direction),
            Rotation::Clockwise180 => Rotation::Clockwise180.rotate(direction),
            Rotation::CounterClockwise90 => Rotation::Clockwise90.rotate(direction),
        }
    }

    const fn property_name_from_direction(direction: Direction) -> Option<&'static str> {
        match direction {
            BlockPropertyDirection::North => Some("north"),
            BlockPropertyDirection::East => Some("east"),
            BlockPropertyDirection::South => Some("south"),
            BlockPropertyDirection::West => Some("west"),
            BlockPropertyDirection::Down | BlockPropertyDirection::Up => None,
        }
    }

    fn rotate_rail_shape(shape: &str, rotation: Rotation) -> Option<&'static str> {
        match rotation {
            Rotation::Clockwise180 => match shape {
                "ascending_east" => Some("ascending_west"),
                "ascending_west" => Some("ascending_east"),
                "ascending_north" => Some("ascending_south"),
                "ascending_south" => Some("ascending_north"),
                "north_south" => Some("north_south"),
                "east_west" => Some("east_west"),
                "south_east" => Some("north_west"),
                "south_west" => Some("north_east"),
                "north_west" => Some("south_east"),
                "north_east" => Some("south_west"),
                _ => None,
            },
            Rotation::CounterClockwise90 => match shape {
                "ascending_east" => Some("ascending_north"),
                "ascending_west" => Some("ascending_south"),
                "ascending_north" => Some("ascending_west"),
                "ascending_south" => Some("ascending_east"),
                "north_south" => Some("east_west"),
                "east_west" => Some("north_south"),
                "south_east" => Some("north_east"),
                "south_west" => Some("south_east"),
                "north_west" => Some("south_west"),
                "north_east" => Some("north_west"),
                _ => None,
            },
            Rotation::Clockwise90 => match shape {
                "ascending_east" => Some("ascending_south"),
                "ascending_west" => Some("ascending_north"),
                "ascending_north" => Some("ascending_east"),
                "ascending_south" => Some("ascending_west"),
                "north_south" => Some("east_west"),
                "east_west" => Some("north_south"),
                "south_east" => Some("south_west"),
                "south_west" => Some("north_west"),
                "north_west" => Some("north_east"),
                "north_east" => Some("south_east"),
                _ => None,
            },
            Rotation::None => None,
        }
    }

    fn mirror_rail_shape(shape: &str, mirror: StructureMirror) -> Option<&'static str> {
        match mirror {
            StructureMirror::LeftRight => match shape {
                "ascending_north" => Some("ascending_south"),
                "ascending_south" => Some("ascending_north"),
                "north_south" => Some("north_south"),
                "east_west" => Some("east_west"),
                "south_east" => Some("north_east"),
                "south_west" => Some("north_west"),
                "north_west" => Some("south_west"),
                "north_east" => Some("south_east"),
                _ => None,
            },
            StructureMirror::FrontBack => match shape {
                "ascending_east" => Some("ascending_west"),
                "ascending_west" => Some("ascending_east"),
                "ascending_north" => Some("ascending_north"),
                "ascending_south" => Some("ascending_south"),
                "north_south" => Some("north_south"),
                "east_west" => Some("east_west"),
                "south_east" => Some("south_west"),
                "south_west" => Some("south_east"),
                "north_west" => Some("north_east"),
                "north_east" => Some("north_west"),
                _ => None,
            },
            StructureMirror::None => None,
        }
    }

    fn parse_stair_shape(shape: &str) -> Option<&'static str> {
        match shape {
            "straight" => Some("straight"),
            "inner_left" => Some("inner_left"),
            "inner_right" => Some("inner_right"),
            "outer_left" => Some("outer_left"),
            "outer_right" => Some("outer_right"),
            _ => None,
        }
    }

    fn mirror_stair_shape(
        direction: Direction,
        shape: &str,
        mirror: StructureMirror,
    ) -> Option<(Direction, &'static str)> {
        match mirror {
            StructureMirror::LeftRight
                if matches!(
                    direction,
                    BlockPropertyDirection::North | BlockPropertyDirection::South
                ) =>
            {
                Some((
                    direction.opposite(),
                    match shape {
                        "outer_left" => "outer_right",
                        "inner_right" => "inner_left",
                        "inner_left" => "inner_right",
                        "outer_right" => "outer_left",
                        "straight" => "straight",
                        _ => return None,
                    },
                ))
            }
            StructureMirror::FrontBack
                if matches!(
                    direction,
                    BlockPropertyDirection::West | BlockPropertyDirection::East
                ) =>
            {
                Some((
                    direction.opposite(),
                    match shape {
                        "outer_left" => "outer_right",
                        "outer_right" => "outer_left",
                        "inner_left" => "inner_left",
                        "inner_right" => "inner_right",
                        "straight" => "straight",
                        _ => return None,
                    },
                ))
            }
            StructureMirror::None | StructureMirror::LeftRight | StructureMirror::FrontBack => None,
        }
    }

    fn block_pos_seed(pos: BlockPos) -> i64 {
        let mut seed = i64::from(pos.x().wrapping_mul(3_129_871))
            ^ i64::from(pos.z()).wrapping_mul(116_129_781)
            ^ i64::from(pos.y());
        seed = seed
            .wrapping_mul(seed)
            .wrapping_mul(42_317_861)
            .wrapping_add(seed.wrapping_mul(11));
        seed >> 16
    }

    fn clamped_lerp_inverse(value: i32, min_dist: i32, max_dist: i32, min: f32, max: f32) -> f32 {
        if min_dist == max_dist {
            return max;
        }
        let delta = ((value - min_dist) as f32 / (max_dist - min_dist) as f32).clamp(0.0, 1.0);
        min + delta * (max - min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::blocks::properties::{DoorHingeSide, SlabType};
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_entities;

    fn test_registry() -> Registry {
        init_test_registry();
        Registry::new_vanilla()
    }

    #[test]
    fn zero_position_with_transform_matches_vanilla_rotation_offsets() {
        let template = StructureTemplate {
            size: IVec3::new(6, 10, 8),
            palettes: Vec::new(),
            entities: Vec::new(),
        };
        let zero = BlockPos::new(100, 64, 200);

        assert_eq!(
            template.zero_position_with_transform(zero, Rotation::None),
            zero
        );
        assert_eq!(
            template.zero_position_with_transform(zero, Rotation::Clockwise90),
            BlockPos::new(107, 64, 200)
        );
        assert_eq!(
            template.zero_position_with_transform(zero, Rotation::Clockwise180),
            BlockPos::new(105, 64, 207)
        );
        assert_eq!(
            template.zero_position_with_transform(zero, Rotation::CounterClockwise90),
            BlockPos::new(100, 64, 205)
        );
    }

    #[test]
    fn bounding_box_with_transform_matches_vanilla_mirror_rotation_pivot() {
        let template = StructureTemplate {
            size: IVec3::new(6, 10, 8),
            palettes: Vec::new(),
            entities: Vec::new(),
        };

        assert_eq!(
            template.bounding_box_with_transform(
                BlockPos::new(100, 64, 200),
                Rotation::Clockwise90,
                StructureMirror::FrontBack,
                BlockPos::new(2, 0, 3),
            ),
            BoundingBox::new(IVec3::new(98, 64, 196), IVec3::new(105, 73, 201))
        );
        assert_eq!(
            template.bounding_box_with_transform(
                BlockPos::new(100, 64, 200),
                Rotation::CounterClockwise90,
                StructureMirror::LeftRight,
                BlockPos::new(2, 0, 3),
            ),
            BoundingBox::new(IVec3::new(92, 64, 200), IVec3::new(99, 73, 205))
        );
    }

    #[test]
    fn block_pos_seed_matches_vanilla_mth_get_seed() {
        assert_eq!(
            StructureTemplate::block_pos_seed(BlockPos::new(12, -3, 45)),
            103_080_484_998_711
        );
    }

    #[test]
    fn village_template_loads_entity_payloads() {
        let registry = test_registry();
        let template = StructureTemplate::load_vanilla(
            &registry,
            &Identifier::vanilla_static("village/plains/villagers/unemployed"),
        )
        .expect("villager template should be bundled");

        assert_eq!(template.entities.len(), 1);
        assert_eq!(
            &template.entities[0].entity_type.key,
            &vanilla_entities::VILLAGER.key
        );
        assert!(template.entities[0].nbt.contains("VillagerData"));
        assert!(!template.entities[0].nbt.contains("id"));
    }

    #[test]
    fn brushable_append_loot_infers_block_entity_without_container_reseed() {
        let registry = test_registry();
        let suspicious_sand = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::SUSPICIOUS_SAND);
        let mut brushable_nbt = NbtCompound::new();
        brushable_nbt.insert("LootTable", "minecraft:archaeology/ocean_ruin_warm");
        brushable_nbt.insert("LootTableSeed", 42_i64);

        let brushable_type = StructureTemplate::block_entity_type_for_nbt_or_state(
            &registry,
            suspicious_sand,
            &brushable_nbt,
        )
        .expect("suspicious sand should infer brushable block entity");

        assert_eq!(
            &brushable_type.key,
            &vanilla_block_entity_types::BRUSHABLE_BLOCK.key
        );
        assert!(!StructureTemplate::should_reseed_template_loot(
            Some(brushable_type),
            &brushable_nbt
        ));

        let mut chest_nbt = NbtCompound::new();
        chest_nbt.insert("id", "minecraft:chest");
        chest_nbt.insert("LootTable", "minecraft:chests/village/village_weaponsmith");
        let chest_type = StructureTemplate::block_entity_type_for_nbt_or_state(
            &registry,
            registry.blocks.get_default_state_id(&vanilla_blocks::CHEST),
            &chest_nbt,
        )
        .expect("chest nbt should resolve block entity type");

        assert!(StructureTemplate::should_reseed_template_loot(
            Some(chest_type),
            &chest_nbt
        ));
    }

    #[test]
    fn entity_position_and_rotation_transform_match_vanilla_offsets() {
        let pos = DVec3::new(1.25, 2.0, 3.75);
        let pivot = BlockPos::new(2, 0, 3);

        assert_eq!(
            StructureTemplate::transform_entity_position(
                pos,
                StructureMirror::FrontBack,
                Rotation::Clockwise90,
                pivot,
            ),
            DVec3::new(2.25, 2.0, 0.75)
        );
        assert_eq!(
            StructureTemplate::transform_entity_rotation(
                (30.0, 10.0),
                StructureMirror::LeftRight,
                Rotation::Clockwise90,
            ),
            (240.0, 10.0)
        );
        assert_eq!(
            StructureTemplate::transform_entity_rotation(
                (30.0, 10.0),
                StructureMirror::FrontBack,
                Rotation::Clockwise90,
            ),
            (60.0, 10.0)
        );
    }

    #[test]
    fn hanging_entity_facing_applies_rotation_before_mirror() {
        let mut nbt = NbtCompound::new();
        nbt.insert(
            "Facing",
            StructureTemplate::entity_facing_value(Direction::North),
        );

        StructureTemplate::transform_entity_additional_nbt(
            &mut nbt,
            StructureMirror::LeftRight,
            Rotation::Clockwise90,
        );

        assert_eq!(
            nbt.byte("Facing"),
            Some(StructureTemplate::entity_facing_value(Direction::East))
        );
    }

    #[test]
    fn mirrored_door_transform_toggles_hinge() {
        let registry = test_registry();
        let door = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::SPRUCE_DOOR);
        let door = registry.blocks.set_property(
            door,
            &BlockStateProperties::HORIZONTAL_FACING,
            Direction::East,
        );
        let door = registry.blocks.set_property(
            door,
            &BlockStateProperties::DOOR_HINGE,
            DoorHingeSide::Left,
        );

        let mirrored = StructureTemplate::transform_state(
            &registry,
            door,
            StructureMirror::FrontBack,
            Rotation::None,
        );

        assert_eq!(
            registry
                .blocks
                .try_get_property(mirrored, &BlockStateProperties::HORIZONTAL_FACING),
            Some(Direction::West),
        );
        assert_eq!(
            registry
                .blocks
                .try_get_property(mirrored, &BlockStateProperties::DOOR_HINGE),
            Some(DoorHingeSide::Right),
        );
    }

    #[test]
    fn jigsaw_replacement_uses_final_state_and_removes_nbt() {
        let registry = test_registry();
        let mut nbt = NbtCompound::new();
        nbt.insert(
            "final_state",
            NbtTag::String("minecraft:oak_stairs[facing=east,half=top]".into()),
        );
        let current = ProcessedBlockInfo {
            template_pos: BlockPos::ZERO,
            world_pos: BlockPos::new(1, 2, 3),
            state: registry
                .blocks
                .get_default_state_id(&vanilla_blocks::JIGSAW),
            nbt: Some(nbt),
        };

        let replaced = StructureTemplate::replace_jigsaw_block(&registry, current)
            .expect("non-structure-void final state should remain");

        assert_eq!(replaced.nbt, None);
        assert_eq!(
            replaced.state,
            StructureTemplate::parse_block_state_string(
                &registry,
                "minecraft:oak_stairs[facing=east,half=top]"
            )
            .expect("test final state should parse")
        );
    }

    #[test]
    fn jigsaw_replacement_accepts_trailing_text_like_vanilla_parser() {
        let registry = test_registry();
        let final_state = "minecraft:acacia_fence[east=false,north=false,south=false,waterlogged=false,west=false]]";
        let expected = "minecraft:acacia_fence[east=false,north=false,south=false,waterlogged=false,west=false]";

        assert_eq!(
            StructureTemplate::parse_block_state_string(&registry, final_state),
            StructureTemplate::parse_block_state_string(&registry, expected)
        );
    }

    #[test]
    fn jigsaw_replacement_drops_structure_void_final_state() {
        let registry = test_registry();
        let mut nbt = NbtCompound::new();
        nbt.insert(
            "final_state",
            NbtTag::String("minecraft:structure_void".into()),
        );
        let current = ProcessedBlockInfo {
            template_pos: BlockPos::ZERO,
            world_pos: BlockPos::new(1, 2, 3),
            state: registry
                .blocks
                .get_default_state_id(&vanilla_blocks::JIGSAW),
            nbt: Some(nbt),
        };

        assert!(StructureTemplate::replace_jigsaw_block(&registry, current).is_none());
    }

    #[test]
    fn structure_block_ignore_modes_match_vanilla_single_variants() {
        let registry = test_registry();
        let structure_block = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::STRUCTURE_BLOCK);
        let air = registry.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let stone = registry.blocks.get_default_state_id(&vanilla_blocks::STONE);

        assert!(StructureBlockIgnore::StructureBlock.ignores(&registry, structure_block));
        assert!(!StructureBlockIgnore::StructureBlock.ignores(&registry, air));
        assert!(StructureBlockIgnore::StructureAndAir.ignores(&registry, structure_block));
        assert!(StructureBlockIgnore::StructureAndAir.ignores(&registry, air));
        assert!(!StructureBlockIgnore::StructureAndAir.ignores(&registry, stone));
    }

    #[test]
    fn block_age_processor_preserves_slab_properties() {
        let registry = test_registry();
        let slab = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::STONE_BRICK_SLAB);
        let slab =
            registry
                .blocks
                .set_property(slab, &BlockStateProperties::SLAB_TYPE, SlabType::Top);
        let current = ProcessedBlockInfo {
            template_pos: BlockPos::ZERO,
            world_pos: BlockPos::new(12, 70, -4),
            state: slab,
            nbt: None,
        };
        let mut random = LegacyRandom::from_seed(1);

        let processed =
            StructureTemplate::process_block_age_with_random(&registry, current, 1.0, &mut random);

        assert_eq!(
            StructureTemplate::block_for_state(&registry, processed.state),
            &vanilla_blocks::MOSSY_STONE_BRICK_SLAB
        );
        assert_eq!(
            registry
                .blocks
                .try_get_property(processed.state, &BlockStateProperties::SLAB_TYPE),
            Some(SlabType::Top),
        );
    }

    #[test]
    fn lava_submerged_processor_keeps_non_full_blocks_as_lava() {
        let registry = test_registry();
        let slab = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::STONE_BRICK_SLAB);
        let current = ProcessedBlockInfo {
            template_pos: BlockPos::ZERO,
            world_pos: BlockPos::new(0, 64, 0),
            state: slab,
            nbt: None,
        };
        let lava = registry.blocks.get_default_state_id(&vanilla_blocks::LAVA);

        let processed = StructureTemplate::process_lava_submerged_block(&registry, lava, current);

        assert_eq!(
            StructureTemplate::block_for_state(&registry, processed.state),
            &vanilla_blocks::LAVA
        );
    }

    #[test]
    fn blackstone_replace_processor_preserves_stair_orientation() {
        let registry = test_registry();
        let stairs = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::STONE_BRICK_STAIRS);
        let stairs =
            registry
                .blocks
                .set_property(stairs, &BlockStateProperties::FACING, Direction::East);
        let stairs = registry
            .blocks
            .set_property(stairs, &BlockStateProperties::HALF, Half::Top);
        let current = ProcessedBlockInfo {
            template_pos: BlockPos::ZERO,
            world_pos: BlockPos::new(0, 64, 0),
            state: stairs,
            nbt: None,
        };

        let processed = StructureTemplate::process_blackstone_replace(&registry, current);

        assert_eq!(
            StructureTemplate::block_for_state(&registry, processed.state),
            &vanilla_blocks::POLISHED_BLACKSTONE_BRICK_STAIRS,
        );
        assert_eq!(
            registry
                .blocks
                .try_get_property(processed.state, &BlockStateProperties::FACING),
            Some(Direction::East),
        );
        assert_eq!(
            registry
                .blocks
                .try_get_property(processed.state, &BlockStateProperties::HALF),
            Some(Half::Top),
        );
    }

    #[test]
    fn data_markers_read_shipwreck_structure_blocks() {
        let registry = test_registry();
        let template = StructureTemplate::load_vanilla(
            &registry,
            &Identifier::vanilla_static("shipwreck/with_mast"),
        )
        .expect("shipwreck template should be bundled");
        let settings = StructurePlaceSettings {
            mirror: StructureMirror::None,
            rotation: Rotation::Clockwise90,
            rotation_pivot: BlockPos::new(4, 0, 15),
            bounding_box: BoundingBox::new(IVec3::new(-64, 0, -64), IVec3::new(64, 128, 64)),
            processors: &[],
            block_ignore: StructureBlockIgnore::StructureAndAir,
            late_block_ignore: StructureBlockIgnore::None,
            replace_jigsaws: false,
            projection: None,
            processor_random: StructureProcessorRandom::Positional,
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
        };
        let mut random = WorldgenRandom::from_seed(0);

        let mut markers = template
            .data_markers(&registry, BlockPos::ZERO, &settings, &mut random)
            .into_iter()
            .map(|marker| marker.metadata)
            .collect::<Vec<_>>();
        markers.sort();

        assert_eq!(markers, ["map_chest", "supply_chest", "treasure_chest"]);
    }

    #[test]
    fn data_markers_read_igloo_chest_structure_block() {
        let registry = test_registry();
        let template =
            StructureTemplate::load_vanilla(&registry, &Identifier::vanilla_static("igloo/bottom"))
                .expect("igloo bottom template should be bundled");
        let settings = StructurePlaceSettings {
            mirror: StructureMirror::None,
            rotation: Rotation::Clockwise180,
            rotation_pivot: BlockPos::new(3, 6, 7),
            bounding_box: BoundingBox::new(IVec3::new(-64, 0, -64), IVec3::new(64, 128, 64)),
            processors: &[],
            block_ignore: StructureBlockIgnore::StructureBlock,
            late_block_ignore: StructureBlockIgnore::None,
            replace_jigsaws: false,
            projection: None,
            processor_random: StructureProcessorRandom::Positional,
            liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
        };
        let mut random = WorldgenRandom::from_seed(0);

        let markers = template
            .data_markers(&registry, BlockPos::ZERO, &settings, &mut random)
            .into_iter()
            .map(|marker| marker.metadata)
            .collect::<Vec<_>>();

        assert_eq!(markers, ["chest"]);
    }
}
