use crate::vanilla_blocks;
use crate::{
    REGISTRY,
    blocks::{
        self, BlockRef, BlockStateTickingMetadata,
        properties::{Direction, Property},
        shapes::{OffsetVoxelShape, SupportType},
    },
    fluid::FluidState,
};
use glam::DVec3;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

pub trait BlockStateExt {
    fn get_block(&self) -> BlockRef;
    fn is_air(&self) -> bool;
    /// Returns Vanilla's immutable cached fluid/random-tick metadata for this state.
    fn get_ticking_metadata(&self) -> BlockStateTickingMetadata;
    /// Mirrors Vanilla's cached `BlockState.getFluidState()`.
    fn get_fluid_state(&self) -> FluidState;
    /// Returns whether the cached fluid state is non-empty.
    fn has_fluid(&self) -> bool;
    /// Mirrors Vanilla's cached `BlockState.isRandomlyTicking()` for the block callback.
    fn is_randomly_ticking(&self) -> bool;
    /// Returns whether this block structurally supports a block entity.
    ///
    /// Extracted Vanilla type memberships match `EntityBlock` exactly. Steel extends that into a
    /// registration contract: plugin blocks must be accepted by at least one registered block
    /// entity type, while the owning block behavior remains responsible for instance creation.
    fn has_block_entity(&self) -> bool;
    fn get_value<P: Property>(&self, property: &P) -> P::Value;
    /// Gets the value of a property, returning `None` if the block doesn't have this property.
    fn try_get_value<P: Property>(&self, property: &P) -> Option<P::Value>;
    #[must_use]
    fn set_value<P: Property>(&self, property: &P, value: P::Value) -> BlockStateId;
    fn get_property_str(&self, name: &str) -> Option<String>;
    fn with_properties_of(&self, source: BlockStateId) -> BlockStateId;
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
    /// Returns the extracted static `BlockState.isRedstoneConductor` value.
    /// Dynamic behavior queries must also receive the live level and position.
    fn is_static_redstone_conductor(&self) -> bool;
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
    fn with_properties_of(&self, source: BlockStateId) -> BlockStateId {
        REGISTRY
            .blocks
            .copy_matching_properties(source, self.get_block())
    }
    fn is_air(&self) -> bool {
        self.get_ticking_metadata().is_air()
    }

    fn get_ticking_metadata(&self) -> BlockStateTickingMetadata {
        let Some(metadata) = REGISTRY.blocks.get_ticking_metadata(*self) else {
            panic!("invalid block state id {}", self.0);
        };
        metadata
    }

    fn get_fluid_state(&self) -> FluidState {
        self.get_ticking_metadata().fluid_state()
    }

    fn has_fluid(&self) -> bool {
        self.get_ticking_metadata().has_fluid()
    }

    fn is_randomly_ticking(&self) -> bool {
        self.get_ticking_metadata().randomly_ticking_block()
    }

    fn has_block_entity(&self) -> bool {
        REGISTRY
            .block_entity_types
            .has_block_entity(self.get_block())
    }

    fn get_value<P: Property>(&self, property: &P) -> P::Value {
        REGISTRY.blocks.get_property(*self, property)
    }

    fn try_get_value<P: Property>(&self, property: &P) -> Option<P::Value> {
        REGISTRY.blocks.try_get_property(*self, property)
    }

    fn set_value<P: Property>(&self, property: &P, value: P::Value) -> BlockStateId {
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

    fn is_static_redstone_conductor(&self) -> bool {
        REGISTRY.blocks.is_static_redstone_conductor(*self)
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
    use crate::{test_support::init_test_registry, vanilla_fluids};
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
    fn static_redstone_conductor_uses_extracted_vanilla_state_predicate() {
        init_test_registry();

        assert!(
            vanilla_blocks::STONE
                .default_state()
                .is_static_redstone_conductor()
        );
        assert!(
            vanilla_blocks::SOUL_SAND
                .default_state()
                .is_static_redstone_conductor()
        );
        assert!(
            !vanilla_blocks::REDSTONE_BLOCK
                .default_state()
                .is_static_redstone_conductor()
        );
        assert!(
            !vanilla_blocks::PISTON
                .default_state()
                .is_static_redstone_conductor()
        );
    }

    #[test]
    fn vanilla_air_variants_are_air() {
        init_test_registry();

        assert!(vanilla_blocks::AIR.default_state().is_air());
        assert!(vanilla_blocks::CAVE_AIR.default_state().is_air());
        assert!(vanilla_blocks::VOID_AIR.default_state().is_air());
    }

    #[test]
    fn block_entity_presence_uses_extracted_type_validity() {
        init_test_registry();

        assert!(
            vanilla_blocks::MOVING_PISTON
                .default_state()
                .has_block_entity()
        );
        assert!(vanilla_blocks::CHEST.default_state().has_block_entity());
        assert!(!vanilla_blocks::STONE.default_state().has_block_entity());
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

    #[test]
    fn with_properties_of_keeps_target_defaults_for_non_matching_properties() {
        init_test_registry();

        let source = vanilla_blocks::STONE.default_state();
        let target = vanilla_blocks::CANDLE.default_state();

        assert_eq!(target.with_properties_of(source), target);
    }

    #[test]
    fn cached_random_tick_metadata_tracks_state_dependent_predicates() {
        init_test_registry();

        let decaying_leaves = vanilla_blocks::OAK_LEAVES.default_state();
        assert!(decaying_leaves.is_randomly_ticking());
        assert!(
            !decaying_leaves
                .set_value(&BlockStateProperties::DISTANCE, 6)
                .is_randomly_ticking()
        );
        assert!(
            !decaying_leaves
                .set_value(&BlockStateProperties::PERSISTENT, true)
                .is_randomly_ticking()
        );

        let immature_wheat = vanilla_blocks::WHEAT.default_state();
        assert!(immature_wheat.is_randomly_ticking());
        assert!(
            !immature_wheat
                .set_value(&BlockStateProperties::AGE_7, 7)
                .is_randomly_ticking()
        );
    }

    #[test]
    fn cached_fluid_state_preserves_source_flowing_and_falling_variants() {
        init_test_registry();

        let wet_slab = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        assert_eq!(
            wet_slab.get_fluid_state(),
            FluidState::source(&vanilla_fluids::WATER)
        );

        let wet_grate = vanilla_blocks::COPPER_GRATE
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        assert_eq!(
            wet_grate.get_fluid_state(),
            FluidState::new(&vanilla_fluids::WATER, 8, true)
        );

        let source_water = vanilla_blocks::WATER.default_state();
        assert_eq!(
            source_water.get_fluid_state(),
            FluidState::source(&vanilla_fluids::WATER)
        );
        assert_eq!(
            source_water
                .set_value(&BlockStateProperties::LEVEL, 1)
                .get_fluid_state(),
            FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 7, false)
        );
        assert_eq!(
            source_water
                .set_value(&BlockStateProperties::LEVEL, 8)
                .get_fluid_state(),
            FluidState::flowing(&vanilla_fluids::FLOWING_WATER, 8, true)
        );
    }

    #[test]
    fn cached_metadata_keeps_block_and_fluid_random_ticks_distinct() {
        init_test_registry();

        let lava = vanilla_blocks::LAVA.default_state().get_ticking_metadata();
        assert!(lava.randomly_ticking_block());
        assert!(lava.randomly_ticking_fluid());

        let water = vanilla_blocks::WATER.default_state().get_ticking_metadata();
        assert!(!water.randomly_ticking_block());
        assert!(!water.randomly_ticking_fluid());
    }
}
