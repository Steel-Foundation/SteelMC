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
    fn get_outline_shape(&self) -> &'static [blocks::shapes::AABB];
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

    fn get_collision_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_collision_shape(*self)
    }

    fn get_outline_shape(&self) -> &'static [blocks::shapes::AABB] {
        REGISTRY.blocks.get_outline_shape(*self)
    }

    fn is_face_sturdy(&self, direction: Direction) -> bool {
        self.is_face_sturdy_for(direction, SupportType::Full)
    }

    fn is_face_sturdy_for(&self, direction: Direction, support_type: SupportType) -> bool {
        let shape = self.get_collision_shape();
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

    fn is_replaceable(&self) -> bool {
        self.get_block().config.replaceable
    }
}

pub trait FluidReplaceableExt {
    fn can_be_replaced_by_fluid(&self, fluid: BlockRef) -> bool;
}

impl FluidReplaceableExt for BlockStateId {
    fn can_be_replaced_by_fluid(&self, fluid: BlockRef) -> bool {
        let block = self.get_block();

        if block == vanilla_blocks::AIR {
            return true;
        }

        if fluid == vanilla_blocks::WATER
            && let Some(false) = self.try_get_value(&BlockStateProperties::WATERLOGGED)
        {
            return true;
        }

        // Vanilla: `state.canBeReplaced() || !state.isSolid()`
        block.config.replaceable || !self.is_solid()
    }
}
