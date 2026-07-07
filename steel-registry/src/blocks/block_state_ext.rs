use crate::vanilla_blocks;
use crate::{
    REGISTRY,
    blocks::{
        self, BlockRef,
        properties::{Direction, Property},
        shapes::{OffsetVoxelShape, SupportType},
    },
};
use glam::DVec3;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

pub trait BlockStateExt {
    fn get_block(&self) -> BlockRef;
    fn is_air(&self) -> bool;
    fn has_block_entity(&self) -> bool;
    fn get_value<T, P: Property<T>>(&self, property: &P) -> T;
    /// Gets the value of a property, returning `None` if the block doesn't have this property.
    fn try_get_value<T, P: Property<T>>(&self, property: &P) -> Option<T>;
    #[must_use]
    fn set_value<T, P: Property<T>>(&self, property: &P, value: T) -> BlockStateId;
    fn get_property_str(&self, name: &str) -> Option<String>;
    fn get_static_collision_shape(&self) -> blocks::shapes::VoxelShape;
    fn get_collision_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape;
    fn get_static_support_shape(&self) -> blocks::shapes::VoxelShape;
    fn get_support_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape;
    fn get_static_outline_shape(&self) -> blocks::shapes::VoxelShape;
    fn get_outline_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape;
    fn get_occlusion_shape(&self) -> blocks::shapes::VoxelShape;
    fn get_static_interaction_shape(&self) -> blocks::shapes::VoxelShape;
    fn get_interaction_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape;
    fn get_static_visual_shape(&self) -> blocks::shapes::VoxelShape;
    fn get_visual_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape;
    /// Returns this block state's block light emission, in vanilla's 0-15 range.
    fn get_light_emission(&self) -> u8;
    /// Returns this block state's light dampening, in vanilla's 0-15 range.
    fn get_light_dampening(&self) -> u8;
    /// Returns true if vanilla uses face shapes for light occlusion on this state.
    fn use_shape_for_light_occlusion(&self) -> bool;
    /// Mirrors vanilla `BlockState.getOffset(BlockPos)`.
    fn get_offset(&self, pos: BlockPos) -> DVec3;
    /// Checks if this block face is sturdy enough to support other blocks.
    /// Uses `SupportType::Full` by default.
    fn is_face_sturdy_at(&self, pos: BlockPos, direction: Direction) -> bool;
    /// Checks if this block face is sturdy for the given support type.
    fn is_face_sturdy_for_at(
        &self,
        pos: BlockPos,
        direction: Direction,
        support_type: SupportType,
    ) -> bool;
    /// Checks if this block state is solid (has a full cube collision shape).
    ///
    /// This matches vanilla's `BlockState.isSolid()` which is used by standing signs
    /// to check if they can be placed on a block.
    fn is_solid(&self) -> bool;
    /// Checks if this block state blocks motion.
    ///
    /// This matches vanilla's `BlockState.blocksMotion()`.
    fn blocks_motion(&self) -> bool;
    /// Checks if this block state renders as a full solid cube.
    ///
    /// This matches vanilla's cached `BlockState.isSolidRender()`, based on the
    /// occlusion shape rather than collision shape.
    fn is_solid_render(&self) -> bool;
    /// Returns vanilla `BlockState.isSuffocating`.
    fn is_suffocating(&self) -> bool;
    /// Returns if a block can be replaced extracted from the minecraft data
    fn is_replaceable(&self) -> bool;
}

impl BlockStateExt for BlockStateId {
    fn get_block(&self) -> BlockRef {
        REGISTRY
            .blocks
            .by_state_id(*self)
            .expect("Expected a valid state id")
    }

    fn is_air(&self) -> bool {
        self.get_block().config.is_air
    }

    fn has_block_entity(&self) -> bool {
        // TODO: Implement when block entities are added
        false
    }

    fn get_value<T, P: Property<T>>(&self, property: &P) -> T {
        REGISTRY.blocks.get_property(*self, property)
    }

    fn try_get_value<T, P: Property<T>>(&self, property: &P) -> Option<T> {
        REGISTRY.blocks.try_get_property(*self, property)
    }

    fn set_value<T, P: Property<T>>(&self, property: &P, value: T) -> BlockStateId {
        REGISTRY.blocks.set_property(*self, property, value)
    }

    fn get_property_str(&self, name: &str) -> Option<String> {
        REGISTRY
            .blocks
            .get_properties(*self)
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.to_string())
    }

    fn get_static_collision_shape(&self) -> blocks::shapes::VoxelShape {
        REGISTRY.blocks.get_static_collision_shape(*self)
    }

    fn get_collision_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape {
        REGISTRY.blocks.get_collision_shape_at(*self, pos)
    }

    fn get_static_support_shape(&self) -> blocks::shapes::VoxelShape {
        REGISTRY.blocks.get_static_support_shape(*self)
    }

    fn get_support_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape {
        REGISTRY.blocks.get_support_shape_at(*self, pos)
    }

    fn get_static_outline_shape(&self) -> blocks::shapes::VoxelShape {
        REGISTRY.blocks.get_static_outline_shape(*self)
    }

    fn get_outline_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape {
        REGISTRY.blocks.get_outline_shape_at(*self, pos)
    }

    fn get_occlusion_shape(&self) -> blocks::shapes::VoxelShape {
        REGISTRY.blocks.get_occlusion_shape(*self)
    }

    fn get_static_interaction_shape(&self) -> blocks::shapes::VoxelShape {
        REGISTRY.blocks.get_static_interaction_shape(*self)
    }

    fn get_interaction_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape {
        REGISTRY.blocks.get_interaction_shape_at(*self, pos)
    }

    fn get_static_visual_shape(&self) -> blocks::shapes::VoxelShape {
        REGISTRY.blocks.get_static_visual_shape(*self)
    }

    fn get_visual_shape_at(&self, pos: BlockPos) -> OffsetVoxelShape {
        REGISTRY.blocks.get_visual_shape_at(*self, pos)
    }

    fn get_light_emission(&self) -> u8 {
        REGISTRY.blocks.get_light_properties(*self).light_emission
    }

    fn get_light_dampening(&self) -> u8 {
        REGISTRY.blocks.get_light_properties(*self).light_dampening
    }

    fn use_shape_for_light_occlusion(&self) -> bool {
        REGISTRY
            .blocks
            .get_light_properties(*self)
            .use_shape_for_light_occlusion
    }

    fn get_offset(&self, pos: BlockPos) -> DVec3 {
        self.get_block().offset_at(pos)
    }

    fn is_face_sturdy_at(&self, pos: BlockPos, direction: Direction) -> bool {
        self.is_face_sturdy_for_at(pos, direction, SupportType::Full)
    }

    fn is_face_sturdy_for_at(
        &self,
        pos: BlockPos,
        direction: Direction,
        support_type: SupportType,
    ) -> bool {
        let shape = self.get_support_shape_at(pos);
        blocks::shapes::is_offset_face_sturdy(shape, direction, support_type)
    }

    fn is_solid(&self) -> bool {
        let block = self.get_block();

        // Check force flags first (matches vanilla's calculateSolid)
        if block.config.force_solid_on {
            return true;
        }
        if block.config.force_solid_off {
            return false;
        }

        // Vanilla's calculateSolid: check collision shape bounding box.
        // A block is solid if its average dimension size >= 35/48 (~0.7292)
        // or its Y size >= 1.0. This catches partial blocks like cactus
        let shape = self.get_static_collision_shape();
        if shape.is_empty() {
            return false;
        }
        let bounds = blocks::shapes::bounding_box(shape);
        bounds.size() >= 0.729_166_7 || bounds.height() >= 1.0
    }

    fn blocks_motion(&self) -> bool {
        let block = self.get_block();
        block != &vanilla_blocks::COBWEB
            && block != &vanilla_blocks::BAMBOO_SAPLING
            && self.is_solid()
    }

    fn is_solid_render(&self) -> bool {
        self.get_block().config.can_occlude
            && blocks::shapes::is_shape_full_block(self.get_occlusion_shape())
    }

    fn is_suffocating(&self) -> bool {
        REGISTRY.blocks.is_suffocating(*self)
    }

    fn is_replaceable(&self) -> bool {
        self.get_block().config.replaceable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::behavior::OffsetType;
    use crate::blocks::properties::BlockStateProperties;
    use crate::blocks::shapes::{ShapeChannel, SupportType};
    use crate::test_support::init_test_registry;
    use steel_utils::Direction;

    #[test]
    fn solid_render_uses_occlusion_shape_not_collision_shape() {
        init_test_registry();

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        assert!(stone.is_solid_render());

        let glass = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::GLASS);
        assert!(blocks::shapes::is_shape_full_block(
            glass.get_static_collision_shape()
        ));
        assert!(!glass.is_solid_render());
    }

    #[test]
    fn light_properties_match_generated_state_offsets() {
        init_test_registry();

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        assert_eq!(air.get_light_emission(), 0);
        assert_eq!(air.get_light_dampening(), 0);
        assert!(!air.use_shape_for_light_occlusion());

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        assert_eq!(stone.get_light_emission(), 0);
        assert_eq!(stone.get_light_dampening(), 15);
        assert!(!stone.use_shape_for_light_occlusion());

        let light = vanilla_blocks::LIGHT.default_state();
        assert_eq!(light.get_light_emission(), 15);
        let dim_light = light.set_value(&BlockStateProperties::LEVEL, 7);
        assert_eq!(dim_light.get_light_emission(), 7);

        let sticky_piston = vanilla_blocks::STICKY_PISTON.default_state();
        assert_eq!(sticky_piston.get_light_dampening(), 15);
        assert!(!sticky_piston.use_shape_for_light_occlusion());

        let extended_piston = sticky_piston.set_value(&BlockStateProperties::EXTENDED, true);
        assert_eq!(extended_piston.get_light_dampening(), 0);
        assert!(extended_piston.use_shape_for_light_occlusion());
    }

    #[test]
    fn blocks_motion_matches_vanilla_base_predicate() {
        init_test_registry();

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        assert!(stone.blocks_motion());

        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        assert!(!water.blocks_motion());

        let cobweb = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::COBWEB);
        assert!(!cobweb.blocks_motion());
    }

    #[test]
    fn suffocating_uses_extracted_vanilla_state_predicate() {
        init_test_registry();

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        assert!(stone.is_suffocating());

        let glass = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::GLASS);
        assert!(glass.blocks_motion());
        assert!(!glass.is_suffocating());

        let farmland = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::FARMLAND);
        assert!(farmland.is_suffocating());
    }

    #[test]
    fn vanilla_air_variants_are_air() {
        init_test_registry();

        assert!(vanilla_blocks::AIR.default_state().is_air());
        assert!(vanilla_blocks::CAVE_AIR.default_state().is_air());
        assert!(vanilla_blocks::VOID_AIR.default_state().is_air());
    }

    #[test]
    fn fence_post_supports_center_attachments_from_below() {
        init_test_registry();

        let fence = vanilla_blocks::OAK_FENCE
            .default_state()
            .set_value(&BlockStateProperties::EAST, true);

        assert!(fence.is_face_sturdy_for_at(BlockPos::ZERO, Direction::Down, SupportType::Center));
    }

    #[test]
    fn generated_shape_offset_flags_distinguish_visual_offset_from_server_shapes() {
        init_test_registry();

        let sulfur_spike = vanilla_blocks::SULFUR_SPIKE.default_state().get_block();
        assert_eq!(sulfur_spike.config.offset_type, OffsetType::Xz);
        assert_eq!(sulfur_spike.config.max_horizontal_offset, 0.125);
        assert!(
            sulfur_spike
                .shape_offsets
                .uses_offset(ShapeChannel::Collision)
        );
        assert!(
            sulfur_spike
                .shape_offsets
                .uses_offset(ShapeChannel::Outline)
        );

        let tall_grass = vanilla_blocks::TALL_GRASS.default_state().get_block();
        assert_eq!(tall_grass.config.offset_type, OffsetType::Xz);
        assert!(
            !tall_grass
                .shape_offsets
                .uses_offset(ShapeChannel::Collision)
        );
        assert!(!tall_grass.shape_offsets.uses_offset(ShapeChannel::Outline));
    }
}
