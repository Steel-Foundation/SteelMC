use crate::vanilla_blocks;
use crate::{
    REGISTRY,
    blocks::{
        self, BlockRef,
        properties::{BlockStateProperties, Direction, Property},
        shapes::SupportType,
    },
};
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
    fn get_collision_shape(&self) -> &'static [blocks::shapes::AABB];
    fn get_support_shape(&self) -> &'static [blocks::shapes::AABB];
    fn get_outline_shape(&self) -> &'static [blocks::shapes::AABB];
    fn get_occlusion_shape(&self) -> &'static [blocks::shapes::AABB];
    fn get_interaction_shape(&self) -> &'static [blocks::shapes::AABB];
    fn get_visual_shape(&self) -> &'static [blocks::shapes::AABB];
    /// Checks if this block face is sturdy enough to support other blocks.
    /// Uses `SupportType::Full` by default.
    fn is_face_sturdy(&self, direction: Direction) -> bool;
    /// Checks if this block face is sturdy for the given support type.
    fn is_face_sturdy_for(&self, direction: Direction, support_type: SupportType) -> bool;
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
    /// Returns if a block can be replaced extracted from the minecraft data
    fn is_replaceable(&self) -> bool;
    /// Returns true if this block state contains fluid — either a liquid block or a waterlogged block.
    /// Mirrors vanilla's `!blockState.getFluidState().isEmpty()`.
    fn has_fluid(&self) -> bool;
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

    fn get_collision_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_collision_shape(*self)
    }

    fn get_support_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_support_shape(*self)
    }

    fn get_outline_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_outline_shape(*self)
    }

    fn get_occlusion_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_occlusion_shape(*self)
    }

    fn get_interaction_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_interaction_shape(*self)
    }

    fn get_visual_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_visual_shape(*self)
    }

    fn is_face_sturdy(&self, direction: Direction) -> bool {
        self.is_face_sturdy_for(direction, SupportType::Full)
    }

    fn is_face_sturdy_for(&self, direction: Direction, support_type: SupportType) -> bool {
        let shape = self.get_support_shape();
        blocks::shapes::is_face_sturdy(shape, direction, support_type)
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
        let shape = self.get_collision_shape();
        if shape.is_empty() {
            return false;
        }
        let bounds = blocks::shapes::bounding_box(shape);
        bounds.get_size() >= 0.729_166_7 || bounds.height() >= 1.0
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

    fn is_replaceable(&self) -> bool {
        self.get_block().config.replaceable
    }

    fn has_fluid(&self) -> bool {
        self.get_block().config.liquid
            || self
                .try_get_value(&BlockStateProperties::WATERLOGGED)
                .unwrap_or(false)
    }
}

pub trait FluidReplaceableExt {
    fn can_be_replaced_by_fluid(&self, fluid: BlockRef) -> bool;
}

impl FluidReplaceableExt for BlockStateId {
    fn can_be_replaced_by_fluid(&self, fluid: BlockRef) -> bool {
        let block = self.get_block();

        if block == &vanilla_blocks::AIR {
            return true;
        }

        if fluid == &vanilla_blocks::WATER
            && let Some(false) = self.try_get_value(&BlockStateProperties::WATERLOGGED)
        {
            return true;
        }

        // Vanilla: `state.canBeReplaced() || !state.isSolid()`
        block.config.replaceable || !self.is_solid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::properties::BlockStateProperties;
    use crate::blocks::shapes::SupportType;
    use crate::test_support::init_test_registry;
    use steel_utils::Direction;

    #[test]
    fn solid_render_uses_occlusion_shape_not_collision_shape() {
        init_test_registry();

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        assert!(stone.is_solid_render());

        let glass = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::GLASS);
        assert!(blocks::shapes::is_shape_full_block(
            glass.get_collision_shape()
        ));
        assert!(!glass.is_solid_render());
    }

    #[test]
    fn blocks_motion_matches_vanilla_base_predicate() {
        init_test_registry();

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        assert!(stone.blocks_motion());

        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        assert!(!water.blocks_motion());
        assert!(water.has_fluid());

        let cobweb = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::COBWEB);
        assert!(!cobweb.blocks_motion());
    }

    #[test]
    fn fence_post_supports_center_attachments_from_below() {
        init_test_registry();

        let fence = vanilla_blocks::OAK_FENCE
            .default_state()
            .set_value(&BlockStateProperties::EAST, true);

        assert!(fence.is_face_sturdy_for(Direction::Down, SupportType::Center));
    }
}
