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

        let shape = self.get_collision_shape();
        if shape.is_empty() {
            return false;
        }

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut min_z = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        let mut max_z = f32::MIN;

        for aabb in shape {
            min_x = min_x.min(aabb.min_x);
            min_y = min_y.min(aabb.min_y);
            min_z = min_z.min(aabb.min_z);
            max_x = max_x.max(aabb.max_x);
            max_y = max_y.max(aabb.max_y);
            max_z = max_z.max(aabb.max_z);
        }

        let dx = (max_x - min_x) as f64;
        let dy = (max_y - min_y) as f64;
        let dz = (max_z - min_z) as f64;
        let volume = dx * dy * dz;

        volume >= 0.7291666666666666 || dy >= 1.0
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

        // Like vanilla's `BlockState.canBeReplaced(Fluid)`
        block.config.replaceable || !self.is_solid()
    }
}

pub trait FluidStateExt {
    fn contains_fluid(&self) -> bool;
}

impl FluidStateExt for BlockStateId {
    fn contains_fluid(&self) -> bool {
        let block = self.get_block();

        // Vanilla-like: water + lava are fluids
        block == vanilla_blocks::WATER || block == vanilla_blocks::LAVA
    }
}
