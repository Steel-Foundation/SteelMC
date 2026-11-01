//! Validates extension-authored properties and shape channels.

use std::collections::BTreeSet;

use steel_registry::{
    Registry, RegistryExt,
    blocks::{
        Block, BlockLightProperties, BlockRef, StateBooleanData,
        behavior::{BlockConfig, OffsetType},
        shapes::{VoxelShape, is_shape_full_block},
        state_data::BlockStateData,
    },
    client_content::{BlockDefinition as NativeDefinition, HorizontalPlacement as NativePlacement},
    sound_types,
};
use steel_utils::{BlockLocalAabb, Identifier};

use crate::content::{
    BlockDefinition, BlockProperties, BlockShapes, BlockStateDefinition, HorizontalPlacement,
    VoxelShape as ApiShape,
};

use super::prepare_displays;

fn shape(shape: ApiShape) -> Result<VoxelShape, String> {
    let mut boxes = Vec::with_capacity(shape.boxes.len());
    for b in shape.boxes {
        if (0..3).any(|axis| {
            !b.min[axis].is_finite() || !b.max[axis].is_finite() || b.min[axis] >= b.max[axis]
        }) {
            return Err("shape boxes require finite, increasing bounds".into());
        }
        boxes.push(BlockLocalAabb::new(
            b.min[0] / 16.0,
            b.min[1] / 16.0,
            b.min[2] / 16.0,
            b.max[0] / 16.0,
            b.max[1] / 16.0,
            b.max[2] / 16.0,
        ));
    }
    // Registry definitions and their shape storage live for the server process.
    Ok(VoxelShape::from_boxes(Box::leak(boxes.into_boxed_slice())))
}

fn properties(config: &mut BlockConfig, p: &BlockProperties) -> Result<(), String> {
    macro_rules! copy { ($($field:ident),* $(,)?) => { $(if let Some(value) = p.$field.as_ref() { config.$field = *value; })* }; }
    copy!(
        destroy_time,
        explosion_resistance,
        friction,
        speed_factor,
        jump_factor,
        requires_correct_tool_for_drops,
        ignited_by_lava,
        replaceable,
        can_occlude
    );
    if !config.destroy_time.is_finite()
        || config.destroy_time < -1.0
        || [
            config.explosion_resistance,
            config.friction,
            config.speed_factor,
            config.jump_factor,
        ]
        .into_iter()
        .any(|value| !value.is_finite() || value < 0.0)
    {
        return Err(
            "block properties require finite, nonnegative values (hardness may also be -1)".into(),
        );
    }
    if let Some(name) = p.sound.as_ref() {
        config.sound_type = sound_types::by_name(name.as_str())
            .ok_or_else(|| format!("unknown extracted sound type {name}"))?;
    }
    config.loot_table = None;
    config.is_randomly_ticking = false;
    config.dynamic_shape = false;
    Ok(())
}

pub(super) fn prepare(
    registry: &Registry,
    definition: BlockDefinition,
) -> Result<NativeDefinition, String> {
    let key = definition
        .key
        .as_str()
        .parse::<Identifier>()
        .map_err(ToString::to_string)?;
    let material = definition
        .properties
        .copy_from
        .as_ref()
        .map(|key| {
            let id = key
                .as_str()
                .parse::<Identifier>()
                .map_err(ToString::to_string)?;
            registry
                .blocks
                .by_key(&id)
                .filter(|block| {
                    registry.client_content.is_vanilla_block(block)
                        && !block.config.is_air
                        && !block.config.liquid
                })
                .ok_or_else(|| format!("unknown or unsupported property source {key}"))
        })
        .transpose()?;
    let mut config = material.map_or_else(BlockConfig::new, |block| block.config.clone());
    properties(&mut config, &definition.properties)?;
    let mut block = Block::new(key, config, &[]);
    let preferred_geometry = geometry(
        registry,
        &mut block,
        definition.vanilla_geometry.match_owned(Some, || None),
        definition.shapes,
    )?;
    if block.get_outline_shape(0).is_empty() {
        return Err("a custom block must have a targeting outline".into());
    }
    block.config.has_collision = !block.get_collision_shape(0).is_empty();
    let full = is_shape_full_block(block.get_collision_shape(0));
    block.suffocating = StateBooleanData::new(full && block.config.can_occlude, &[]);
    block.redstone_conductor = StateBooleanData::new(full, &[]);
    let old = block.get_light_properties(0);
    let light_emission =
        Option::from(definition.properties.light_emission).unwrap_or(old.light_emission);
    let light_dampening =
        Option::from(definition.properties.light_dampening).unwrap_or(old.light_dampening);
    if light_emission > 15 || light_dampening > 15 {
        return Err("block light values must be between 0 and 15".into());
    }
    block.light_properties = BlockStateData::Fixed(BlockLightProperties {
        light_emission,
        light_dampening,
        use_shape_for_light_occlusion: old.use_shape_for_light_occlusion,
    });
    let displays = prepare_displays(registry, definition.displays.as_slice())?;
    Ok(NativeDefinition {
        block,
        mining_material: material,
        preferred_geometry,
        allow_approximation: definition.allow_approximation,
        horizontal: definition.horizontal.match_owned(
            |placement| {
                Some(match placement {
                    HorizontalPlacement::Player => NativePlacement::Player,
                    HorizontalPlacement::AgainstClickedFace => NativePlacement::AgainstClickedFace,
                })
            },
            || None,
        ),
        displays,
    })
}

fn geometry(
    registry: &Registry,
    block: &mut Block,
    source: Option<BlockStateDefinition>,
    shapes: BlockShapes,
) -> Result<Option<BlockRef>, String> {
    if let Some(source) = source {
        let id = source
            .block
            .as_str()
            .parse::<Identifier>()
            .map_err(ToString::to_string)?;
        let geometry = registry
            .blocks
            .by_key(&id)
            .filter(|b| registry.client_content.is_vanilla_block(b))
            .ok_or("unknown vanilla geometry")?;
        let mut names = BTreeSet::new();
        if source
            .properties
            .iter()
            .any(|p| !names.insert(p.name.as_str()))
        {
            return Err("duplicate geometry property".into());
        }
        let state = registry
            .blocks
            .state_id_from_block_defaulted_properties(
                geometry,
                source
                    .properties
                    .iter()
                    .map(|p| (p.name.as_str(), p.value.as_str())),
            )
            .ok_or("invalid geometry properties")?;
        let offset = state.0 - registry.blocks.get_base_state_id(geometry).0;
        if geometry.config.offset_type != OffsetType::None
            || geometry.config.dynamic_shape
            || geometry.config.is_air
            || geometry.config.liquid
            || registry.block_entity_types.has_block_entity(geometry)
            || !geometry.fluid_state.value(offset).is_empty()
            || geometry.get_outline_shape(offset).is_empty()
        {
            return Err(
                "geometry must be static, targetable, dry, and without a block entity".into(),
            );
        }
        block.collision_shape = BlockStateData::Fixed(geometry.get_collision_shape(offset));
        block.support_shape = BlockStateData::Fixed(geometry.get_support_shape(offset));
        block.outline_shape = BlockStateData::Fixed(geometry.get_outline_shape(offset));
        block.occlusion_shape = BlockStateData::Fixed(geometry.get_occlusion_shape(offset));
        block.interaction_shape = BlockStateData::Fixed(geometry.get_interaction_shape(offset));
        block.visual_shape = BlockStateData::Fixed(geometry.get_visual_shape(offset));
        block.light_properties = BlockStateData::Fixed(geometry.get_light_properties(offset));
        block.shape_offsets = geometry.shape_offsets;
        block.config.offset_type = geometry.config.offset_type;
        block.config.max_horizontal_offset = geometry.config.max_horizontal_offset;
        block.config.max_vertical_offset = geometry.config.max_vertical_offset;
        return Ok(Some(geometry));
    }
    let s = shapes;
    block.collision_shape = BlockStateData::Fixed(shape(s.collision)?);
    block.support_shape = BlockStateData::Fixed(shape(s.support)?);
    block.outline_shape = BlockStateData::Fixed(shape(s.outline)?);
    block.occlusion_shape = BlockStateData::Fixed(shape(s.occlusion)?);
    block.interaction_shape = BlockStateData::Fixed(shape(s.interaction)?);
    block.visual_shape = BlockStateData::Fixed(shape(s.visual)?);
    Ok(None)
}
