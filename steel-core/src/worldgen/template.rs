use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::str::FromStr;

use flate2::read::GzDecoder;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::properties::Direction as BlockPropertyDirection;
use steel_registry::blocks::{self};
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _};
use steel_registry::shared_structs::BlockStateData;
use steel_registry::structure_processor::{
    PosRuleTestData, ProcessorRuleData, RuleBlockEntityModifierData, StructureProcessorAxis,
    StructureProcessorKind, StructureRuleTestData,
};
use steel_registry::{Registry, RegistryExt, TaggedRegistryExt, vanilla_template_pools};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{
    BlockPos, BlockStateId, BoundingBox, Direction, Identifier, Rotation, types::UpdateFlags,
};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::state_resolver::WorldgenStateResolver;

/// Loaded vanilla structure template payload.
///
/// Steel keeps template data separate from template-pool metadata. Pools only need jigsaw
/// summaries during structure-start planning; feature and piece placement need the full NBT
/// block payload and processors, so this type mirrors vanilla's loaded `StructureTemplate`.
#[derive(Debug, Clone)]
pub(crate) struct StructureTemplate {
    size: [i32; 3],
    palettes: Vec<StructureTemplatePalette>,
    entity_count: usize,
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
struct ProcessedBlockInfo {
    template_pos: BlockPos,
    world_pos: BlockPos,
    state: BlockStateId,
    nbt: Option<NbtCompound>,
}

pub(crate) struct StructurePlaceSettings<'a> {
    pub(crate) rotation: Rotation,
    pub(crate) bounding_box: BoundingBox,
    pub(crate) processors: &'a [StructureProcessorKind],
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

        let nbt = simdnbt::borrow::read(&mut Cursor::new(&data))
            .map_err(|err| format!("failed to parse structure template {context}: {err}"))?;
        let root = match nbt {
            simdnbt::borrow::Nbt::Some(root) => root,
            simdnbt::borrow::Nbt::None => {
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

        let entity_count = compound
            .list("entities")
            .and_then(|list| list.compounds())
            .map_or(0, |entities| entities.len());

        Ok(Self {
            size,
            palettes: loaded_palettes,
            entity_count,
        })
    }

    fn read_vec3(
        list: Option<simdnbt::borrow::NbtList<'_, '_>>,
        context: &str,
        field: &str,
    ) -> Result<[i32; 3], String> {
        let ints = list
            .and_then(|list| list.ints())
            .ok_or_else(|| format!("structure template {context} has non-int {field} list"))?;
        if ints.len() < 3 {
            return Err(format!(
                "structure template {context} {field} list has fewer than 3 entries"
            ));
        }
        Ok([ints[0], ints[1], ints[2]])
    }

    fn read_palettes(
        registry: &Registry,
        compound: &simdnbt::borrow::NbtCompound<'_, '_>,
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
        entries: &simdnbt::borrow::NbtCompoundList<'_, '_>,
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
        blocks: &simdnbt::borrow::NbtCompoundList<'_, '_>,
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

    fn is_static_full_block(registry: &Registry, state: BlockStateId) -> bool {
        let Some(block) = registry.blocks.by_state_id(state) else {
            return false;
        };
        !block.config.dynamic_shape
            && blocks::shapes::is_shape_full_block(registry.blocks.get_collision_shape(state))
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

    pub(crate) fn size(&self, rotation: Rotation) -> [i32; 3] {
        let (x, y, z) = rotation.rotate_size(self.size[0], self.size[1], self.size[2]);
        [x, y, z]
    }

    pub(crate) fn zero_position_with_transform(
        &self,
        zero_pos: BlockPos,
        rotation: Rotation,
    ) -> BlockPos {
        let x = self.size[0] - 1;
        let z = self.size[2] - 1;
        match rotation {
            Rotation::None => zero_pos,
            Rotation::Clockwise90 => zero_pos.offset(z, 0, 0),
            Rotation::Clockwise180 => zero_pos.offset(x, 0, z),
            Rotation::CounterClockwise90 => zero_pos.offset(0, 0, x),
        }
    }

    pub(crate) fn bounding_box(&self, position: BlockPos, rotation: Rotation) -> BoundingBox {
        rotation.get_bounding_box(
            position.x(),
            position.y(),
            position.z(),
            self.size[0],
            self.size[1],
            self.size[2],
        )
    }

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
        let Some(palette) = self.palette(random) else {
            return false;
        };
        let mut processed_blocks = Vec::with_capacity(palette.blocks.len());

        for block in &palette.blocks {
            let original = ProcessedBlockInfo {
                template_pos: block.pos,
                world_pos: Self::transformed_position(position, block.pos, settings.rotation),
                state: block.state,
                nbt: block.nbt.clone(),
            };

            let Some(processed) =
                Self::process_block(region, registry, &original, settings, reference_pos, random)
            else {
                continue;
            };

            if !settings.bounding_box.is_inside(processed.world_pos) {
                continue;
            }

            processed_blocks.push(processed);
        }

        let mut placed_any = false;
        let mut placed_positions = Vec::with_capacity(processed_blocks.len());
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut min_z = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let mut max_z = i32::MIN;
        for processed in processed_blocks {
            let final_state = Self::rotate_state(registry, processed.state, settings.rotation);
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

            if let Some(nbt) = processed.nbt {
                Self::place_block_entity(region, registry, processed.world_pos, final_state, nbt);
            } else {
                let _ = region.remove_block_entity(processed.world_pos);
            }
        }

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

        if self.entity_count != 0 {
            // TODO: Place structure template entities when structure pieces use full template placement.
        }

        placed_any
    }

    fn update_shape_at_edge(
        region: &mut WorldGenRegion<'_>,
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

    fn palette(&self, random: &mut WorldgenRandom) -> Option<&StructureTemplatePalette> {
        if self.palettes.is_empty() {
            return None;
        }
        let Ok(bound) = i32::try_from(self.palettes.len()) else {
            panic!(
                "structure template palette count {} exceeds i32 range",
                self.palettes.len()
            );
        };
        Some(&self.palettes[random.next_i32_bounded(bound) as usize])
    }

    fn transformed_position(
        position: BlockPos,
        template_pos: BlockPos,
        rotation: Rotation,
    ) -> BlockPos {
        let (x, y, z) =
            rotation.transform_pos(template_pos.x(), template_pos.y(), template_pos.z(), 0, 0);
        position.offset(x, y, z)
    }

    fn process_block(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        original: &ProcessedBlockInfo,
        settings: &StructurePlaceSettings<'_>,
        reference_pos: BlockPos,
        random: &mut WorldgenRandom,
    ) -> Option<ProcessedBlockInfo> {
        let mut current = original.clone();
        for processor in settings.processors {
            current = Self::process_block_with_processor(
                region,
                registry,
                processor,
                original,
                current,
                reference_pos,
                random,
            )?;
        }
        Some(current)
    }

    fn process_block_with_processor(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        processor: &StructureProcessorKind,
        original: &ProcessedBlockInfo,
        current: ProcessedBlockInfo,
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
                (random.next_f32() <= *integrity).then_some(current)
            }
            StructureProcessorKind::ProtectedBlocks { cannot_replace } => {
                let existing =
                    Self::block_for_state(registry, region.block_state(current.world_pos));
                (!registry.blocks.is_in_tag(existing, cannot_replace)).then_some(current)
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
            StructureProcessorKind::Capped { .. } => {
                // TODO: Implement vanilla CappedProcessor finalization before using capped processor
                // lists for structure-piece placement. Fossil processor lists do not contain capped.
                Some(current)
            }
        }
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
                let mut nbt = current.nbt.unwrap_or_else(NbtCompound::new);
                nbt.insert("LootTable", NbtTag::String(loot_table.to_string().into()));
                nbt.insert("LootTableSeed", NbtTag::Long(random.next_i64()));
                Some(nbt)
            }
        };
        current
    }

    fn place_block_entity(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
        state: BlockStateId,
        nbt: NbtCompound,
    ) {
        let Some(id) = nbt.string("id") else {
            // TODO: Infer block entity type from the placed block state once block entity type
            // ownership is stored on blocks. Vanilla creates the block entity during setBlock.
            return;
        };
        let Ok(id) = Identifier::from_str(id.to_str().as_ref()) else {
            return;
        };
        let Some(block_entity_type) = registry.block_entity_types.by_key(&id) else {
            return;
        };
        let _ = region.set_block_entity_data(pos, block_entity_type, state, nbt);
    }

    fn rotate_state(registry: &Registry, state: BlockStateId, rotation: Rotation) -> BlockStateId {
        if rotation == Rotation::None {
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
                        "x" => *value = "z".to_owned(),
                        "z" => *value = "x".to_owned(),
                        _ => {}
                    }
                }
                "facing" => {
                    if let Some(direction) = Self::parse_direction(value) {
                        *value = rotation.rotate(direction).as_str().to_owned();
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
                "north" | "east" | "south" | "west" => {
                    let from = Self::direction_from_property_name(name);
                    let source = Self::inverse_rotate_direction(rotation, from);
                    if let Some(source_name) = Self::property_name_from_direction(source)
                        && let Some((_, source_value)) = original
                            .iter()
                            .find(|(original_name, _)| original_name == source_name)
                    {
                        *value = source_value.clone();
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
            "north" => BlockPropertyDirection::North,
            "east" => BlockPropertyDirection::East,
            "south" => BlockPropertyDirection::South,
            "west" => BlockPropertyDirection::West,
            _ => BlockPropertyDirection::North,
        }
    }

    fn inverse_rotate_direction(rotation: Rotation, direction: Direction) -> Direction {
        match rotation {
            Rotation::None => direction,
            Rotation::Clockwise90 => Rotation::CounterClockwise90.rotate(direction),
            Rotation::Clockwise180 => Rotation::Clockwise180.rotate(direction),
            Rotation::CounterClockwise90 => Rotation::Clockwise90.rotate(direction),
        }
    }

    fn property_name_from_direction(direction: Direction) -> Option<&'static str> {
        match direction {
            BlockPropertyDirection::North => Some("north"),
            BlockPropertyDirection::East => Some("east"),
            BlockPropertyDirection::South => Some("south"),
            BlockPropertyDirection::West => Some("west"),
            BlockPropertyDirection::Down | BlockPropertyDirection::Up => None,
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

    #[test]
    fn zero_position_with_transform_matches_vanilla_rotation_offsets() {
        let template = StructureTemplate {
            size: [6, 10, 8],
            palettes: Vec::new(),
            entity_count: 0,
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
    fn block_pos_seed_matches_vanilla_mth_get_seed() {
        assert_eq!(
            StructureTemplate::block_pos_seed(BlockPos::new(12, -3, 45)),
            103_080_484_998_711
        );
    }
}
