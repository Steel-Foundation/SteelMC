use std::sync::Weak;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{Registry, vanilla_block_entity_types, vanilla_blocks};
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockPos, BlockStateId, BoundingBox, Direction, types::UpdateFlags};

use crate::behavior::BlockStateBehaviorExt as _;
use crate::chunk::heightmap::HeightmapType;
use crate::entity::SharedEntity;
use crate::world::World;
use crate::worldgen::region::WorldGenRegion;
use crate::worldgen::template::StructureTemplate;

use super::StructurePiecePlacer;

pub(super) struct ScatteredFeaturePlacer<'a, 'world> {
    region: &'a mut WorldGenRegion<'world>,
    registry: &'a Registry,
    bounding_box: &'a mut BoundingBox,
    orientation: Option<Direction>,
    clip: BoundingBox,
}

impl<'a, 'world> ScatteredFeaturePlacer<'a, 'world> {
    pub(super) const fn new(
        region: &'a mut WorldGenRegion<'world>,
        registry: &'a Registry,
        bounding_box: &'a mut BoundingBox,
        orientation: Option<Direction>,
        clip: BoundingBox,
    ) -> Self {
        Self {
            region,
            registry,
            bounding_box,
            orientation,
            clip,
        }
    }

    pub(super) fn update_average_ground_height(
        &mut self,
        height_position: &mut Option<i32>,
        offset: i32,
    ) -> bool {
        if height_position.is_some() {
            return true;
        }

        let mut total = 0;
        let mut count = 0;
        for z in self.bounding_box.min_z..=self.bounding_box.max_z {
            for x in self.bounding_box.min_x..=self.bounding_box.max_x {
                if self.clip.is_inside(BlockPos::new(x, 64, z)) {
                    total += self
                        .region
                        .height_at(HeightmapType::MotionBlockingNoLeaves, x, z);
                    count += 1;
                }
            }
        }

        if count == 0 {
            return false;
        }

        let adjusted = total / count;
        *height_position = Some(adjusted);
        let dy = adjusted - self.bounding_box.min_y + offset;
        self.bounding_box.min_y += dy;
        self.bounding_box.max_y += dy;
        true
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla StructurePiece.generateBox parameters"
    )]
    pub(super) fn generate_box(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: BlockStateId,
        fill: BlockStateId,
        skip_air: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if skip_air && self.get_block(x, y, z).is_air() {
                        continue;
                    }
                    let state = if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                        fill
                    } else {
                        edge
                    };
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla StructurePiece.generateBox selector overload"
    )]
    pub(super) fn generate_box_with_selector(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        skip_air: bool,
        random: &mut WorldgenRandom,
        mut selector: impl FnMut(&mut WorldgenRandom, i32, i32, i32, bool) -> BlockStateId,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if skip_air && self.get_block(x, y, z).is_air() {
                        continue;
                    }
                    let is_edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    let state = selector(random, x, y, z, is_edge);
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    pub(super) fn generate_air_box(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
    ) {
        let air = vanilla_blocks::AIR.default_state();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    self.place_block(air, x, y, z);
                }
            }
        }
    }

    pub(super) fn fill_column_down(&mut self, state: BlockStateId, x: i32, start_y: i32, z: i32) {
        let mut pos = self.world_pos(x, start_y, z);
        if !self.clip.is_inside(pos) {
            return;
        }

        while Self::is_replaceable_by_structures(self.region.block_state(pos))
            && pos.y() > self.region.min_y() + 1
        {
            let _ = self
                .region
                .set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
            pos = pos.below();
        }
    }

    pub(super) fn place_block(&mut self, state: BlockStateId, x: i32, y: i32, z: i32) {
        let pos = self.world_pos(x, y, z);
        if !self.clip.is_inside(pos) {
            return;
        }

        let state = self.transform_state(state);
        let _ = self
            .region
            .set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
        if StructurePiecePlacer::needs_structure_shape_postprocessing(state) {
            self.region.mark_pos_for_postprocessing(pos);
        }
        let fluid_state = state.get_fluid_state();
        if !fluid_state.is_empty() {
            let _ = self
                .region
                .schedule_fluid_tick_default(pos, fluid_state.fluid_id, 0);
        }
    }

    pub(super) fn create_chest(
        &mut self,
        random: &mut WorldgenRandom,
        x: i32,
        y: i32,
        z: i32,
        loot_table: &'static str,
    ) -> bool {
        let pos = self.world_pos(x, y, z);
        StructurePiecePlacer::create_loot_chest(self.region, self.clip, random, pos, loot_table)
    }

    pub(super) fn create_dispenser(
        &mut self,
        random: &mut WorldgenRandom,
        x: i32,
        y: i32,
        z: i32,
        facing: Direction,
        loot_table: &'static str,
    ) -> bool {
        let pos = self.world_pos(x, y, z);
        if !self.clip.is_inside(pos)
            || self.region.block_state(pos).get_block() == &vanilla_blocks::DISPENSER
        {
            return false;
        }

        let state = self.transform_state(
            vanilla_blocks::DISPENSER
                .default_state()
                .set_value(&BlockStateProperties::FACING, facing),
        );
        if !self
            .region
            .set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS)
        {
            return false;
        }
        if StructurePiecePlacer::needs_structure_shape_postprocessing(state) {
            self.region.mark_pos_for_postprocessing(pos);
        }

        StructurePiecePlacer::set_loot_table_block_entity(
            self.region,
            pos,
            &vanilla_block_entity_types::DISPENSER,
            state,
            loot_table,
            random.next_i64(),
        )
    }

    pub(super) fn create_spawner(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        entity_id: &'static str,
    ) -> bool {
        let pos = self.world_pos(x, y, z);
        if !self.clip.is_inside(pos) {
            return false;
        }

        let spawner = vanilla_blocks::SPAWNER.default_state();
        if !self
            .region
            .set_block_state(pos, spawner, UpdateFlags::UPDATE_CLIENTS)
        {
            return false;
        }
        StructurePiecePlacer::set_spawner_entity(self.region, pos, spawner, entity_id)
    }

    pub(super) const fn world_pos(&self, x: i32, y: i32, z: i32) -> BlockPos {
        let world_y = if self.orientation.is_some() {
            y + self.bounding_box.min_y
        } else {
            y
        };
        let (world_x, world_z) = match self.orientation {
            None | Some(Direction::Up | Direction::Down) => (x, z),
            Some(Direction::North) => (self.bounding_box.min_x + x, self.bounding_box.max_z - z),
            Some(Direction::South) => (self.bounding_box.min_x + x, self.bounding_box.min_z + z),
            Some(Direction::West) => (self.bounding_box.max_x - z, self.bounding_box.min_z + x),
            Some(Direction::East) => (self.bounding_box.min_x + z, self.bounding_box.min_z + x),
        };
        BlockPos::new(world_x, world_y, world_z)
    }

    pub(super) const fn clip(&self) -> BoundingBox {
        self.clip
    }

    pub(super) fn sea_level(&self) -> i32 {
        self.region.sea_level()
    }

    pub(super) fn block_at(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        self.get_block(x, y, z)
    }

    pub(super) fn chunk_intersects(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let pos0 = self.world_pos(x0, 0, z0);
        let pos1 = self.world_pos(x1, 0, z1);
        self.clip.intersects_xz(
            pos0.x().min(pos1.x()),
            pos0.z().min(pos1.z()),
            pos0.x().max(pos1.x()),
            pos0.z().max(pos1.z()),
        )
    }

    pub(super) fn weak_world(&self) -> Weak<World> {
        self.region.weak_world()
    }

    pub(super) fn add_fresh_entity(&mut self, entity: SharedEntity) -> bool {
        self.region.add_fresh_entity(entity)
    }

    fn get_block(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        let pos = self.world_pos(x, y, z);
        if self.clip.is_inside(pos) {
            self.region.block_state(pos)
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn transform_state(&self, state: BlockStateId) -> BlockStateId {
        let (mirror, rotation) = StructurePiecePlacer::orientation_transform(self.orientation);
        StructureTemplate::transform_state(self.registry, state, mirror, rotation)
    }

    fn is_replaceable_by_structures(state: BlockStateId) -> bool {
        state.is_air()
            || state.has_fluid()
            || state.get_block() == &vanilla_blocks::GLOW_LICHEN
            || state.get_block() == &vanilla_blocks::SEAGRASS
            || state.get_block() == &vanilla_blocks::TALL_SEAGRASS
    }
}
