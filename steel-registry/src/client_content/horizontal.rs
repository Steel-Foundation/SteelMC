//! Facing states rotate authored north geometry and models together.

use std::f32::consts::{FRAC_PI_2, PI};

use glam::{Quat, Vec3};
use steel_utils::{BlockLocalAabb, BlockStateId, Direction};

use crate::{
    Registry,
    blocks::{
        Block,
        properties::{BlockStateProperties, Property},
        shapes::VoxelShape,
        state_data::BlockStateData,
    },
};

use super::BlockItemDisplay;

/// Custom placement rules; neither rule adds stair corners or upside-down states.
#[derive(Debug, Clone, Copy)]
pub enum HorizontalPlacement {
    /// The same horizontal facing used when placing vanilla stairs.
    Player,
    /// Wall clicks put the solid back against the clicked face. Other clicks use player facing.
    AgainstClickedFace,
}

#[derive(Debug)]
pub struct ClientHorizontalState {
    pub carrier: BlockStateId,
    pub displays: Vec<BlockItemDisplay>,
}

#[derive(Debug)]
pub struct ClientHorizontal {
    pub placement: HorizontalPlacement,
    pub states: Vec<ClientHorizontalState>,
}

/// A clockwise rotation around the block's center, viewed from above.
#[derive(Clone, Copy)]
struct HorizontalRotation(Direction);

impl HorizontalRotation {
    fn point(self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        match self.0 {
            Direction::East => [1.0 - z, y, x],
            Direction::South => [1.0 - x, y, 1.0 - z],
            Direction::West => [z, y, 1.0 - x],
            _ => [x, y, z],
        }
    }

    fn shape(self, shape: VoxelShape) -> VoxelShape {
        if self.0 == Direction::North {
            return shape;
        }
        let boxes: Vec<_> = shape
            .boxes()
            .iter()
            .map(|b| {
                let a = self.point([b.min_x(), b.min_y(), b.min_z()]);
                let b = self.point([b.max_x(), b.max_y(), b.max_z()]);
                BlockLocalAabb::new(
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].min(b[2]),
                    a[0].max(b[0]),
                    a[1].max(b[1]),
                    a[2].max(b[2]),
                )
            })
            .collect();
        VoxelShape::from_boxes(Box::leak(boxes.into_boxed_slice()))
    }

    fn displays(self, displays: &[BlockItemDisplay]) -> Vec<BlockItemDisplay> {
        let angle = match self.0 {
            Direction::East => -FRAC_PI_2,
            Direction::South => PI,
            Direction::West => FRAC_PI_2,
            _ => 0.0,
        };
        let rotation = Quat::from_rotation_y(angle);
        displays
            .iter()
            .map(|part| {
                let p = self.point(part.translation.as_dvec3().to_array());
                BlockItemDisplay {
                    item: part.item.clone(),
                    translation: Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32),
                    scale: part.scale,
                    rotation: rotation * part.rotation,
                }
            })
            .collect()
    }
}

impl Registry {
    pub(super) fn horizontal_definition(
        &mut self,
        block: &mut Block,
        displays: &[BlockItemDisplay],
        preferred: Option<crate::blocks::BlockRef>,
        approximate: bool,
        placement: HorizontalPlacement,
    ) -> Result<ClientHorizontal, String> {
        let facing = &BlockStateProperties::HORIZONTAL_FACING;
        let mut states = Vec::new();
        let mut collision = Vec::new();
        let mut support = Vec::new();
        let mut outline = Vec::new();
        let mut occlusion = Vec::new();
        let mut interaction = Vec::new();
        let mut visual = Vec::new();
        for direction in facing.get_possible_values() {
            let rotation = HorizontalRotation(direction);
            let c = rotation.shape(block.get_collision_shape(0));
            let o = rotation.shape(block.get_outline_shape(0));
            let carrier = self.select_client_state(c, o, preferred, approximate)?;
            collision.push(c);
            outline.push(o);
            support.push(rotation.shape(block.get_support_shape(0)));
            occlusion.push(rotation.shape(block.get_occlusion_shape(0)));
            interaction.push(rotation.shape(block.get_interaction_shape(0)));
            visual.push(rotation.shape(block.get_visual_shape(0)));
            states.push(ClientHorizontalState {
                carrier,
                displays: rotation.displays(displays),
            });
        }
        block.properties = &[&BlockStateProperties::HORIZONTAL_FACING];
        block.default_state_offset = facing.get_internal_index(&Direction::North) as u16;
        block.collision_shape = BlockStateData::Values(Box::leak(collision.into_boxed_slice()));
        block.support_shape = BlockStateData::Values(Box::leak(support.into_boxed_slice()));
        block.outline_shape = BlockStateData::Values(Box::leak(outline.into_boxed_slice()));
        block.occlusion_shape = BlockStateData::Values(Box::leak(occlusion.into_boxed_slice()));
        block.interaction_shape = BlockStateData::Values(Box::leak(interaction.into_boxed_slice()));
        block.visual_shape = BlockStateData::Values(Box::leak(visual.into_boxed_slice()));
        Ok(ClientHorizontal { placement, states })
    }
}
