use super::{
    Arc, BlockEntityTypeRef, BlockPos, CBlockDestruction, ChunkPos, DVec3, ItemEntity, ItemStack,
    NbtCompound, RegistryEntry, SectionPos, World,
};

/// Generates a random value using triangle distribution.
///
/// Mirrors vanilla's `RandomSource.triangle(mode, deviation)`.
/// Produces values centered around `mode` with a spread of `deviation`.
fn triangle_random(mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rand::random::<f64>() - rand::random::<f64>())
}

impl World {
    /// Broadcasts block destruction progress to nearby players.
    ///
    /// Note: The packet is NOT sent to the player doing the breaking (matching vanilla).
    /// The breaking player sees progress through client-side prediction.
    ///
    /// # Arguments
    /// * `entity_id` - The entity ID of the player breaking the block
    /// * `pos` - The position of the block being broken
    /// * `progress` - The destruction progress (0-9), or -1 to clear
    #[expect(
        clippy::cast_sign_loss,
        reason = "value is clamped to -1..=9 before cast; -1 wraps intentionally to 255 as sentinel"
    )]
    pub fn broadcast_block_destruction(&self, entity_id: i32, pos: BlockPos, progress: i32) {
        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );
        let packet = CBlockDestruction {
            id: entity_id,
            pos,
            progress: progress.clamp(-1, 9) as u8,
        };
        self.broadcast_to_nearby(chunk, packet, Some(entity_id));
    }

    /// Broadcasts a block entity update to all players tracking the chunk.
    ///
    /// This is used when block entity data changes (e.g., sign text updated).
    ///
    /// # Arguments
    /// * `pos` - The position of the block entity
    /// * `block_entity_type` - The type of block entity
    /// * `nbt` - The NBT data to send
    pub fn broadcast_block_entity_update(
        &self,
        pos: BlockPos,
        block_entity_type: BlockEntityTypeRef,
        nbt: NbtCompound,
    ) {
        use steel_protocol::packets::game::CBlockEntityData;
        use steel_utils::serial::OptionalNbt;

        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        );

        // Get the block entity type ID from the registry
        let type_id = block_entity_type.id();

        let packet = CBlockEntityData {
            pos,
            block_entity_type: type_id as i32,
            nbt: OptionalNbt(Some(nbt)),
        };

        self.broadcast_to_nearby(chunk, packet, None);
    }

    /// Broadcasts the current block-entity update packet when that entity type
    /// exposes client-visible update data.
    pub(crate) fn broadcast_block_entity_if_needed(&self, pos: BlockPos) {
        let Some(block_entity) = self.get_block_entity(pos) else {
            return;
        };
        let update = block_entity
            .get_update_tag()
            .map(|tag| (block_entity.get_type(), tag));
        if let Some((block_entity_type, tag)) = update {
            self.broadcast_block_entity_update(pos, block_entity_type, tag);
        }
    }

    /// Drops an item stack at the given position with scatter behavior.
    ///
    /// Mirrors vanilla's `Containers.dropItemStack`. Splits large stacks into
    /// multiple item entities (10-30 items each) and scatters them with random
    /// positions and velocities.
    ///
    /// # Arguments
    /// * `pos` - The block position to drop the item at
    /// * `item` - The item stack to drop
    pub fn drop_item_stack(self: &Arc<Self>, pos: BlockPos, mut item: ItemStack) {
        use crate::entity::next_entity_id;
        use steel_registry::vanilla_entities;

        // Random velocity using triangle distribution (vanilla uses random.triangle)
        // Vanilla constant: 0.05F * Mth.SQRT_OF_TWO (sqrt(2) * 0.05 ≈ 0.1148...)
        const VELOCITY_SPREAD: f64 = 0.114_850_001_711_398_36;

        if item.is_empty() {
            return;
        }

        // Vanilla uses EntityType.ITEM dimensions for position calculation
        let item_width = f64::from(vanilla_entities::ITEM.dimensions.width);
        let center_range = 1.0 - item_width;
        let half_size = item_width / 2.0;

        // Keep spawning item entities until the stack is empty
        // Vanilla splits stacks into 10-30 items each
        while !item.is_empty() {
            // Split off 10-30 items (or remaining if less)
            let split_count = (rand::random::<u32>() % 21 + 10) as i32;
            let split_stack = item.split(split_count);

            if split_stack.is_empty() {
                break;
            }

            // Random position within the block (vanilla logic)
            let x = f64::from(pos.x()).floor() + rand::random::<f64>() * center_range + half_size;
            let y = f64::from(pos.y()).floor() + rand::random::<f64>() * center_range;
            let z = f64::from(pos.z()).floor() + rand::random::<f64>() * center_range + half_size;

            // triangle(mode, deviation) produces values centered around mode with spread of deviation
            let vx = triangle_random(0.0, VELOCITY_SPREAD);
            let vy = triangle_random(0.2, VELOCITY_SPREAD);
            let vz = triangle_random(0.0, VELOCITY_SPREAD);

            let entity_id = next_entity_id();
            let entity = Arc::new(ItemEntity::with_item_and_velocity(
                &vanilla_entities::ITEM,
                entity_id,
                DVec3::new(x, y, z),
                split_stack,
                DVec3::new(vx, vy, vz),
                Arc::downgrade(self),
            ));
            entity.set_default_pickup_delay();
            if let Err(error) = self.try_add_entity(entity) {
                log::warn!("Failed to drop item stack entity: {error}");
            }
        }
    }
}
