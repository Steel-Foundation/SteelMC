use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_biome_tags::BiomeTag;
use steel_registry::{Registry, RegistryExt, vanilla_blocks, vanilla_entities};
use steel_utils::axis::Axis;
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockPos, BlockStateId, BoundingBox, Direction, Identifier, types::UpdateFlags};

use super::StructurePiecePlacer;
use crate::chunk::heightmap::HeightmapType;
use crate::entity::{entities::ChestMinecartEntity, next_entity_id};
use crate::worldgen::generators::vanilla::fuzzed_biome_at_block;
use crate::worldgen::region::WorldGenRegion;
use steel_worldgen::structure::mineshaft::{
    MineshaftPieceKind, MineshaftPiecePayload, MineshaftType,
};

const ABANDONED_MINESHAFT_LOOT: Identifier =
    Identifier::new_static("minecraft", "chests/abandoned_mineshaft");
const CAVE_SPIDER_ENTITY: &str = "minecraft:cave_spider";

impl StructurePiecePlacer {
    #[expect(
        clippy::too_many_arguments,
        reason = "structure-piece placement carries vanilla postProcess inputs"
    )]
    pub(super) fn place_mineshaft_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounding_box: BoundingBox,
        orientation: Option<Direction>,
        data: &mut MineshaftPiecePayload,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
        biome_zoom_seed: i64,
    ) -> bool {
        let mineshaft_type = data.mineshaft_type;
        let mut placer = MineshaftPlacer {
            region,
            registry,
            bounding_box,
            orientation,
            clip,
            mineshaft_type,
            biome_zoom_seed,
        };
        if placer.is_in_invalid_location() {
            return false;
        }

        match &mut data.kind {
            MineshaftPieceKind::Room {
                child_entrance_boxes,
            } => placer.place_room(child_entrance_boxes),
            MineshaftPieceKind::Corridor {
                has_rails,
                spider_corridor,
                has_placed_spider,
                num_sections,
            } => placer.place_corridor(
                random,
                mineshaft_type,
                *has_rails,
                *spider_corridor,
                has_placed_spider,
                *num_sections,
            ),
            MineshaftPieceKind::Crossing { is_two_floored, .. } => {
                placer.place_crossing(mineshaft_type, *is_two_floored);
            }
            MineshaftPieceKind::Stairs => placer.place_stairs(),
        }
        true
    }
}

struct MineshaftPlacer<'a, 'world> {
    region: &'a mut WorldGenRegion<'world>,
    registry: &'a Registry,
    bounding_box: BoundingBox,
    orientation: Option<Direction>,
    clip: BoundingBox,
    mineshaft_type: MineshaftType,
    biome_zoom_seed: i64,
}

impl MineshaftPlacer<'_, '_> {
    fn place_room(&mut self, child_entrance_boxes: &[BoundingBox]) {
        self.generate_box(
            self.bounding_box.min_x(),
            self.bounding_box.min_y() + 1,
            self.bounding_box.min_z(),
            self.bounding_box.max_x(),
            (self.bounding_box.min_y() + 3).min(self.bounding_box.max_y()),
            self.bounding_box.max_z(),
            Self::cave_air(),
            Self::cave_air(),
            false,
        );

        for entrance_box in child_entrance_boxes {
            self.generate_box(
                entrance_box.min_x(),
                entrance_box.max_y() - 2,
                entrance_box.min_z(),
                entrance_box.max_x(),
                entrance_box.max_y(),
                entrance_box.max_z(),
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
        }

        self.generate_upper_half_sphere(
            self.bounding_box.min_x(),
            self.bounding_box.min_y() + 4,
            self.bounding_box.min_z(),
            self.bounding_box.max_x(),
            self.bounding_box.max_y(),
            self.bounding_box.max_z(),
            Self::cave_air(),
            false,
        );
    }

    fn place_corridor(
        &mut self,
        random: &mut WorldgenRandom,
        mineshaft_type: MineshaftType,
        has_rails: bool,
        spider_corridor: bool,
        has_placed_spider: &mut bool,
        num_sections: i32,
    ) {
        let length = num_sections * 5 - 1;
        let planks = Self::planks_state(mineshaft_type);
        self.generate_box(
            0,
            0,
            0,
            2,
            1,
            length,
            Self::cave_air(),
            Self::cave_air(),
            false,
        );
        self.generate_maybe_box(
            random,
            0.8,
            0,
            2,
            0,
            2,
            2,
            length,
            Self::cave_air(),
            Self::cave_air(),
            false,
            false,
        );
        if spider_corridor {
            self.generate_maybe_box(
                random,
                0.6,
                0,
                0,
                0,
                2,
                1,
                length,
                Self::cobweb(),
                Self::cave_air(),
                false,
                true,
            );
        }

        for section in 0..num_sections {
            let z = 2 + section * 5;
            self.place_support(random, mineshaft_type, 0, 0, z, 2, 2);
            self.maybe_place_cobweb(random, 0.1, 0, 2, z - 1);
            self.maybe_place_cobweb(random, 0.1, 2, 2, z - 1);
            self.maybe_place_cobweb(random, 0.1, 0, 2, z + 1);
            self.maybe_place_cobweb(random, 0.1, 2, 2, z + 1);
            self.maybe_place_cobweb(random, 0.05, 0, 2, z - 2);
            self.maybe_place_cobweb(random, 0.05, 2, 2, z - 2);
            self.maybe_place_cobweb(random, 0.05, 0, 2, z + 2);
            self.maybe_place_cobweb(random, 0.05, 2, 2, z + 2);

            if random.next_i32_bounded(100) == 0 {
                self.create_chest(random, 2, 0, z - 1);
            }
            if random.next_i32_bounded(100) == 0 {
                self.create_chest(random, 0, 0, z + 1);
            }

            if spider_corridor && !*has_placed_spider {
                let new_z = z - 1 + random.next_i32_bounded(3);
                let pos = self.world_pos(1, 0, new_z);
                if self.clip.contains_blockpos(pos) && self.is_interior(1, 0, new_z) {
                    *has_placed_spider = true;
                    let spawner = Self::spawner();
                    let _ = self
                        .region
                        .set_block_state(pos, spawner, UpdateFlags::UPDATE_CLIENTS);
                    self.set_spawner_entity(pos, spawner, CAVE_SPIDER_ENTITY);
                }
            }
        }

        for x in 0..=2 {
            for z in 0..=length {
                self.set_planks_block(planks, x, -1, z);
            }
        }

        self.place_double_lower_or_upper_support(mineshaft_type, 0, -1, 2);
        if num_sections > 1 {
            self.place_double_lower_or_upper_support(mineshaft_type, 0, -1, length - 2);
        }

        if has_rails {
            let rail =
                Self::rail().set_value(&BlockStateProperties::RAIL_SHAPE, RailShape::NorthSouth);
            for z in 0..=length {
                let floor = self.get_block(1, -1, z);
                if !floor.is_air() && floor.is_solid_render() {
                    let probability = if self.is_interior(1, 0, z) { 0.7 } else { 0.9 };
                    self.maybe_generate_block(random, probability, 1, 0, z, rail);
                }
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "crossing placement follows vanilla's imperative piece layout"
    )]
    fn place_crossing(&mut self, mineshaft_type: MineshaftType, is_two_floored: bool) {
        let planks = Self::planks_state(mineshaft_type);
        if is_two_floored {
            self.generate_box(
                self.bounding_box.min_x() + 1,
                self.bounding_box.min_y(),
                self.bounding_box.min_z(),
                self.bounding_box.max_x() - 1,
                self.bounding_box.min_y() + 2,
                self.bounding_box.max_z(),
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
            self.generate_box(
                self.bounding_box.min_x(),
                self.bounding_box.min_y(),
                self.bounding_box.min_z() + 1,
                self.bounding_box.max_x(),
                self.bounding_box.min_y() + 2,
                self.bounding_box.max_z() - 1,
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
            self.generate_box(
                self.bounding_box.min_x() + 1,
                self.bounding_box.max_y() - 2,
                self.bounding_box.min_z(),
                self.bounding_box.max_x() - 1,
                self.bounding_box.max_y(),
                self.bounding_box.max_z(),
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
            self.generate_box(
                self.bounding_box.min_x(),
                self.bounding_box.max_y() - 2,
                self.bounding_box.min_z() + 1,
                self.bounding_box.max_x(),
                self.bounding_box.max_y(),
                self.bounding_box.max_z() - 1,
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
            self.generate_box(
                self.bounding_box.min_x() + 1,
                self.bounding_box.min_y() + 3,
                self.bounding_box.min_z() + 1,
                self.bounding_box.max_x() - 1,
                self.bounding_box.min_y() + 3,
                self.bounding_box.max_z() - 1,
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
        } else {
            self.generate_box(
                self.bounding_box.min_x() + 1,
                self.bounding_box.min_y(),
                self.bounding_box.min_z(),
                self.bounding_box.max_x() - 1,
                self.bounding_box.max_y(),
                self.bounding_box.max_z(),
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
            self.generate_box(
                self.bounding_box.min_x(),
                self.bounding_box.min_y(),
                self.bounding_box.min_z() + 1,
                self.bounding_box.max_x(),
                self.bounding_box.max_y(),
                self.bounding_box.max_z() - 1,
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
        }

        self.place_support_pillar(
            mineshaft_type,
            self.bounding_box.min_x() + 1,
            self.bounding_box.min_y(),
            self.bounding_box.min_z() + 1,
            self.bounding_box.max_y(),
        );
        self.place_support_pillar(
            mineshaft_type,
            self.bounding_box.min_x() + 1,
            self.bounding_box.min_y(),
            self.bounding_box.max_z() - 1,
            self.bounding_box.max_y(),
        );
        self.place_support_pillar(
            mineshaft_type,
            self.bounding_box.max_x() - 1,
            self.bounding_box.min_y(),
            self.bounding_box.min_z() + 1,
            self.bounding_box.max_y(),
        );
        self.place_support_pillar(
            mineshaft_type,
            self.bounding_box.max_x() - 1,
            self.bounding_box.min_y(),
            self.bounding_box.max_z() - 1,
            self.bounding_box.max_y(),
        );

        let y = self.bounding_box.min_y() - 1;
        for x in self.bounding_box.min_x()..=self.bounding_box.max_x() {
            for z in self.bounding_box.min_z()..=self.bounding_box.max_z() {
                self.set_planks_block(planks, x, y, z);
            }
        }
    }

    fn place_stairs(&mut self) {
        self.generate_box(0, 5, 0, 2, 7, 1, Self::cave_air(), Self::cave_air(), false);
        self.generate_box(0, 0, 7, 2, 2, 8, Self::cave_air(), Self::cave_air(), false);
        for i in 0..5 {
            let min_y = 5 - i - i32::from(i < 4);
            self.generate_box(
                0,
                min_y,
                2 + i,
                2,
                7 - i,
                2 + i,
                Self::cave_air(),
                Self::cave_air(),
                false,
            );
        }
    }

    fn is_in_invalid_location(&self) -> bool {
        let x0 = (self.bounding_box.min_x() - 1).max(self.clip.min_x());
        let y0 = (self.bounding_box.min_y() - 1).max(self.clip.min_y());
        let z0 = (self.bounding_box.min_z() - 1).max(self.clip.min_z());
        let x1 = (self.bounding_box.max_x() + 1).min(self.clip.max_x());
        let y1 = (self.bounding_box.max_y() + 1).min(self.clip.max_y());
        let z1 = (self.bounding_box.max_z() + 1).min(self.clip.max_z());

        let biome_pos = BlockPos::new(
            i32::midpoint(x0, x1),
            i32::midpoint(y0, y1),
            i32::midpoint(z0, z1),
        );
        if self.is_mineshaft_blocking_biome(biome_pos) {
            return true;
        }

        for x in x0..=x1 {
            for z in z0..=z1 {
                if self.is_liquid_block(BlockPos::new(x, y0, z))
                    || self.is_liquid_block(BlockPos::new(x, y1, z))
                {
                    return true;
                }
            }
        }

        for x in x0..=x1 {
            for y in y0..=y1 {
                if self.is_liquid_block(BlockPos::new(x, y, z0))
                    || self.is_liquid_block(BlockPos::new(x, y, z1))
                {
                    return true;
                }
            }
        }

        for z in z0..=z1 {
            for y in y0..=y1 {
                if self.is_liquid_block(BlockPos::new(x0, y, z))
                    || self.is_liquid_block(BlockPos::new(x1, y, z))
                {
                    return true;
                }
            }
        }

        false
    }

    fn is_mineshaft_blocking_biome(&self, pos: BlockPos) -> bool {
        let biome_id = fuzzed_biome_at_block(self.biome_zoom_seed, pos, |quart| {
            self.region.noise_biome_id(quart.x, quart.y, quart.z)
        });
        let Some(biome) = self.registry.biomes.by_id(usize::from(biome_id)) else {
            panic!("noise biome id {biome_id} is not registered");
        };
        biome.has_tag(&BiomeTag::MINESHAFT_BLOCKING)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "box fill helpers use vanilla min/max coordinate signatures"
    )]
    fn generate_box(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge_block: BlockStateId,
        fill_block: BlockStateId,
        skip_air: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if skip_air && self.get_block(x, y, z).is_air() {
                        continue;
                    }
                    let state = if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                        fill_block
                    } else {
                        edge_block
                    };
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors StructurePiece.generateMaybeBox"
    )]
    fn generate_maybe_box(
        &mut self,
        random: &mut WorldgenRandom,
        probability: f32,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge_block: BlockStateId,
        fill_block: BlockStateId,
        skip_air: bool,
        has_to_be_inside: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if random.next_f32() > probability
                        || skip_air && self.get_block(x, y, z).is_air()
                        || has_to_be_inside && !self.is_interior(x, y, z)
                    {
                        continue;
                    }
                    let state = if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                        fill_block
                    } else {
                        edge_block
                    };
                    self.place_block(state, x, y, z);
                }
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors StructurePiece.generateUpperHalfSphere"
    )]
    fn generate_upper_half_sphere(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        fill_block: BlockStateId,
        skip_air: bool,
    ) {
        let diag_x = (x1 - x0 + 1) as f32;
        let diag_y = (y1 - y0 + 1) as f32;
        let diag_z = (z1 - z0 + 1) as f32;
        let cx = x0 as f32 + diag_x / 2.0;
        let cz = z0 as f32 + diag_z / 2.0;

        for y in y0..=y1 {
            let normalized_y = (y - y0) as f32 / diag_y;
            for x in x0..=x1 {
                let normalized_x = (x as f32 - cx) / (diag_x * 0.5);
                for z in z0..=z1 {
                    let normalized_z = (z as f32 - cz) / (diag_z * 0.5);
                    if skip_air && self.get_block(x, y, z).is_air() {
                        continue;
                    }
                    let distance = normalized_x * normalized_x
                        + normalized_y * normalized_y
                        + normalized_z * normalized_z;
                    if distance <= 1.05 {
                        self.place_block(fill_block, x, y, z);
                    }
                }
            }
        }
    }

    fn maybe_generate_block(
        &mut self,
        random: &mut WorldgenRandom,
        probability: f32,
        x: i32,
        y: i32,
        z: i32,
        state: BlockStateId,
    ) {
        if random.next_f32() < probability {
            self.place_block(state, x, y, z);
        }
    }

    fn maybe_place_cobweb(
        &mut self,
        random: &mut WorldgenRandom,
        probability: f32,
        x: i32,
        y: i32,
        z: i32,
    ) {
        if self.is_interior(x, y, z)
            && random.next_f32() < probability
            && self.has_sturdy_neighbors(x, y, z, 2)
        {
            self.place_block(Self::cobweb(), x, y, z);
        }
    }

    fn create_chest(&mut self, random: &mut WorldgenRandom, x: i32, y: i32, z: i32) -> bool {
        let pos = self.world_pos(x, y, z);
        if !self.clip.contains_blockpos(pos)
            || !self.block_state(pos).is_air()
            || self.block_state(pos.below()).is_air()
        {
            return false;
        }

        let shape = if random.next_bool() {
            RailShape::NorthSouth
        } else {
            RailShape::EastWest
        };
        let rail = Self::rail().set_value(&BlockStateProperties::RAIL_SHAPE, shape);
        self.place_block(rail, x, y, z);
        let loot_seed = random.next_i64();
        let chest = Arc::new(ChestMinecartEntity::new(
            &vanilla_entities::CHEST_MINECART,
            next_entity_id(),
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            self.region.weak_world(),
        ));
        chest.set_loot_table(ABANDONED_MINESHAFT_LOOT, loot_seed);
        let _ = self.region.add_fresh_entity(chest);
        true
    }

    fn set_spawner_entity(&mut self, pos: BlockPos, state: BlockStateId, entity_id: &'static str) {
        let _ = StructurePiecePlacer::set_spawner_entity(self.region, pos, state, entity_id);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "support placement follows vanilla coordinate parameters"
    )]
    fn place_support(
        &mut self,
        random: &mut WorldgenRandom,
        mineshaft_type: MineshaftType,
        x0: i32,
        y0: i32,
        z: i32,
        y1: i32,
        x1: i32,
    ) {
        if !self.is_supporting_box(x0, x1, y1, z) {
            return;
        }

        let planks = Self::planks_state(mineshaft_type);
        let fence = Self::fence_state(mineshaft_type);
        self.generate_box(
            x0,
            y0,
            z,
            x0,
            y1 - 1,
            z,
            fence.set_value(&BlockStateProperties::WEST, true),
            Self::cave_air(),
            false,
        );
        self.generate_box(
            x1,
            y0,
            z,
            x1,
            y1 - 1,
            z,
            fence.set_value(&BlockStateProperties::EAST, true),
            Self::cave_air(),
            false,
        );
        if random.next_i32_bounded(4) == 0 {
            self.generate_box(x0, y1, z, x0, y1, z, planks, Self::cave_air(), false);
            self.generate_box(x1, y1, z, x1, y1, z, planks, Self::cave_air(), false);
        } else {
            self.generate_box(x0, y1, z, x1, y1, z, planks, Self::cave_air(), false);
            self.maybe_generate_block(
                random,
                0.05,
                x0 + 1,
                y1,
                z - 1,
                Self::wall_torch().set_value(&BlockStateProperties::FACING, Direction::South),
            );
            self.maybe_generate_block(
                random,
                0.05,
                x0 + 1,
                y1,
                z + 1,
                Self::wall_torch().set_value(&BlockStateProperties::FACING, Direction::North),
            );
        }
    }

    fn is_supporting_box(&self, x0: i32, x1: i32, y1: i32, z: i32) -> bool {
        for x in x0..=x1 {
            if self.get_block(x, y1 + 1, z).is_air() {
                return false;
            }
        }
        true
    }

    fn has_sturdy_neighbors(&self, x: i32, y: i32, z: i32, count: i32) -> bool {
        let pos = self.world_pos(x, y, z);
        let mut sturdy_neighbors = 0;
        for direction in [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            let neighbor = pos.relative(direction);
            if self.clip.contains_blockpos(neighbor)
                && self
                    .block_state(neighbor)
                    .is_face_sturdy_at(neighbor, direction.opposite())
            {
                sturdy_neighbors += 1;
                if sturdy_neighbors >= count {
                    return true;
                }
            }
        }
        false
    }

    fn place_double_lower_or_upper_support(
        &mut self,
        mineshaft_type: MineshaftType,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let wood = Self::wood_state(mineshaft_type);
        let planks = Self::planks_state(mineshaft_type);
        if self.get_block(x, y, z).get_block() == planks.get_block() {
            self.fill_pillar_down_or_chain_up(mineshaft_type, wood, x, y, z);
        }
        if self.get_block(x + 2, y, z).get_block() == planks.get_block() {
            self.fill_pillar_down_or_chain_up(mineshaft_type, wood, x + 2, y, z);
        }
    }

    fn fill_pillar_down_or_chain_up(
        &mut self,
        mineshaft_type: MineshaftType,
        pillar_state: BlockStateId,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let pos = self.world_pos(x, y, z);
        if !self.clip.contains_blockpos(pos) {
            return;
        }

        let world_y = pos.y();
        let mut check_below = true;
        let mut check_above = true;
        let mut distance = 1;
        while check_below || check_above {
            if check_below {
                let below_pos = BlockPos::new(pos.x(), world_y - distance, pos.z());
                let below_state = self.block_state(below_pos);
                let empty_below = Self::is_replaceable_by_structures(below_state)
                    && below_state.get_block() != &vanilla_blocks::LAVA;
                if !empty_below && Self::can_place_column_on_top_of(below_state, below_pos) {
                    self.fill_column_between(
                        pillar_state,
                        pos.x(),
                        pos.z(),
                        world_y - distance + 1,
                        world_y,
                    );
                    return;
                }
                check_below =
                    distance <= 20 && empty_below && below_pos.y() > self.region.min_y() + 1;
            }

            if check_above {
                let above_pos = BlockPos::new(pos.x(), world_y + distance, pos.z());
                let above_state = self.block_state(above_pos);
                let empty_above = Self::is_replaceable_by_structures(above_state);
                if !empty_above && Self::can_hang_chain_below(above_state, above_pos) {
                    let fence_pos = BlockPos::new(pos.x(), world_y + 1, pos.z());
                    let _ = self.region.set_block_state(
                        fence_pos,
                        Self::fence_state(mineshaft_type),
                        UpdateFlags::UPDATE_CLIENTS,
                    );
                    self.fill_column_between(
                        Self::chain(),
                        pos.x(),
                        pos.z(),
                        world_y + 2,
                        world_y + distance,
                    );
                    return;
                }
                check_above =
                    distance <= 50 && empty_above && above_pos.y() < self.region.max_y_exclusive();
            }

            distance += 1;
        }
    }

    fn fill_column_between(
        &mut self,
        state: BlockStateId,
        x: i32,
        z: i32,
        bottom_inclusive: i32,
        top_exclusive: i32,
    ) {
        for y in bottom_inclusive..top_exclusive {
            let _ = self.region.set_block_state(
                BlockPos::new(x, y, z),
                state,
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn can_place_column_on_top_of(state_below: BlockStateId, pos_below: BlockPos) -> bool {
        state_below.is_face_sturdy_at(pos_below, Direction::Up)
    }

    fn can_hang_chain_below(state_above: BlockStateId, pos_above: BlockPos) -> bool {
        state_above.is_face_sturdy_for_at(pos_above, Direction::Down, SupportType::Center)
            && !Self::is_falling_block(state_above)
    }

    fn place_support_pillar(
        &mut self,
        mineshaft_type: MineshaftType,
        x: i32,
        y0: i32,
        z: i32,
        y1: i32,
    ) {
        if !self.get_block(x, y1 + 1, z).is_air() {
            self.generate_box(
                x,
                y0,
                z,
                x,
                y1,
                z,
                Self::planks_state(mineshaft_type),
                Self::cave_air(),
                false,
            );
        }
    }

    fn set_planks_block(&mut self, planks: BlockStateId, x: i32, y: i32, z: i32) {
        if !self.is_interior(x, y, z) {
            return;
        }
        let pos = self.world_pos(x, y, z);
        let existing = self.block_state(pos);
        if !existing.is_face_sturdy_at(pos, Direction::Up) {
            let _ = self
                .region
                .set_block_state(pos, planks, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    fn is_interior(&self, x: i32, y: i32, z: i32) -> bool {
        let pos = self.world_pos(x, y + 1, z);
        self.clip.contains_blockpos(pos)
            && pos.y()
                < self
                    .region
                    .height_at(HeightmapType::OceanFloorWg, pos.x(), pos.z())
    }

    fn place_block(&mut self, state: BlockStateId, x: i32, y: i32, z: i32) {
        let pos = self.world_pos(x, y, z);
        if !self.clip.contains_blockpos(pos) || !self.can_be_replaced(x, y, z) {
            return;
        }

        let state = self.transform_state(state);
        let _ = self
            .region
            .set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS);
        if Self::needs_shape_postprocessing(state) {
            self.region.mark_pos_for_postprocessing(pos);
        }
    }

    fn get_block(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        let pos = self.world_pos(x, y, z);
        if self.clip.contains_blockpos(pos) {
            self.block_state(pos)
        } else {
            Self::air()
        }
    }

    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        if self.region.is_outside_build_height(pos.y()) {
            Self::air()
        } else {
            self.region.block_state(pos)
        }
    }

    fn is_liquid_block(&self, pos: BlockPos) -> bool {
        self.block_state(pos).get_block().config.liquid
    }

    fn can_be_replaced(&self, x: i32, y: i32, z: i32) -> bool {
        let state = self.get_block(x, y, z);
        let block = state.get_block();
        block != Self::planks_state(self.mineshaft_type).get_block()
            && block != Self::wood_state(self.mineshaft_type).get_block()
            && block != Self::fence_state(self.mineshaft_type).get_block()
            && block != &vanilla_blocks::IRON_CHAIN
    }

    fn is_replaceable_by_structures(state: BlockStateId) -> bool {
        state.is_air()
            || state.get_block().config.liquid
            || state.get_block() == &vanilla_blocks::GLOW_LICHEN
            || state.get_block() == &vanilla_blocks::SEAGRASS
            || state.get_block() == &vanilla_blocks::TALL_SEAGRASS
    }

    const fn world_pos(&self, x: i32, y: i32, z: i32) -> BlockPos {
        let world_y = if self.orientation.is_some() {
            y + self.bounding_box.min_y()
        } else {
            y
        };
        let (world_x, world_z) = match self.orientation {
            Some(Direction::North) => {
                (self.bounding_box.min_x() + x, self.bounding_box.max_z() - z)
            }
            Some(Direction::South) => {
                (self.bounding_box.min_x() + x, self.bounding_box.min_z() + z)
            }
            Some(Direction::West) => (self.bounding_box.max_x() - z, self.bounding_box.min_z() + x),
            Some(Direction::East) => (self.bounding_box.min_x() + z, self.bounding_box.min_z() + x),
            None | Some(Direction::Up | Direction::Down) => (x, z),
        };
        BlockPos::new(world_x, world_y, world_z)
    }

    fn transform_state(&self, mut state: BlockStateId) -> BlockStateId {
        if let Some(facing) = state.try_get_value(&BlockStateProperties::FACING) {
            state = state.set_value(
                &BlockStateProperties::FACING,
                self.transform_direction(facing),
            );
        } else if let Some(facing) = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING) {
            state = state.set_value(
                &BlockStateProperties::HORIZONTAL_FACING,
                self.transform_direction(facing),
            );
        }
        if let Some(shape) = state.try_get_value(&BlockStateProperties::RAIL_SHAPE) {
            state = state.set_value(
                &BlockStateProperties::RAIL_SHAPE,
                self.transform_rail_shape(shape),
            );
        }
        self.transform_side_bools(state)
    }

    fn transform_side_bools(&self, state: BlockStateId) -> BlockStateId {
        let sides = [
            (
                Direction::North,
                state.try_get_value(&BlockStateProperties::NORTH),
            ),
            (
                Direction::East,
                state.try_get_value(&BlockStateProperties::EAST),
            ),
            (
                Direction::South,
                state.try_get_value(&BlockStateProperties::SOUTH),
            ),
            (
                Direction::West,
                state.try_get_value(&BlockStateProperties::WEST),
            ),
        ];
        if sides.iter().all(|(_, value)| value.is_none()) {
            return state;
        }

        let mut transformed = state;
        for direction in [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            transformed = Self::set_side(transformed, direction, false);
        }
        for (direction, value) in sides {
            if value == Some(true) {
                transformed =
                    Self::set_side(transformed, self.transform_direction(direction), true);
            }
        }
        transformed
    }

    fn set_side(state: BlockStateId, direction: Direction, value: bool) -> BlockStateId {
        match direction {
            Direction::North => state.set_value(&BlockStateProperties::NORTH, value),
            Direction::East => state.set_value(&BlockStateProperties::EAST, value),
            Direction::South => state.set_value(&BlockStateProperties::SOUTH, value),
            Direction::West => state.set_value(&BlockStateProperties::WEST, value),
            Direction::Up | Direction::Down => state,
        }
    }

    const fn transform_direction(&self, direction: Direction) -> Direction {
        let mirrored = match self.orientation {
            Some(Direction::South | Direction::West) => Self::mirror_left_right(direction),
            _ => direction,
        };
        match self.orientation {
            Some(Direction::West | Direction::East) => mirrored.rotate_y_clockwise(),
            _ => mirrored,
        }
    }

    const fn mirror_left_right(direction: Direction) -> Direction {
        match direction {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            other => other,
        }
    }

    const fn transform_rail_shape(&self, shape: RailShape) -> RailShape {
        match shape {
            RailShape::NorthSouth => match self.transform_direction(Direction::North).axis() {
                Axis::X => RailShape::EastWest,
                _ => RailShape::NorthSouth,
            },
            RailShape::EastWest => match self.transform_direction(Direction::East).axis() {
                Axis::Z => RailShape::NorthSouth,
                _ => RailShape::EastWest,
            },
            other => other,
        }
    }

    fn air() -> BlockStateId {
        vanilla_blocks::AIR.default_state()
    }

    fn cave_air() -> BlockStateId {
        vanilla_blocks::CAVE_AIR.default_state()
    }

    fn cobweb() -> BlockStateId {
        vanilla_blocks::COBWEB.default_state()
    }

    fn rail() -> BlockStateId {
        vanilla_blocks::RAIL.default_state()
    }

    fn spawner() -> BlockStateId {
        vanilla_blocks::SPAWNER.default_state()
    }

    fn chain() -> BlockStateId {
        vanilla_blocks::IRON_CHAIN.default_state()
    }

    fn wall_torch() -> BlockStateId {
        vanilla_blocks::WALL_TORCH.default_state()
    }

    fn wood_state(mineshaft_type: MineshaftType) -> BlockStateId {
        match mineshaft_type {
            MineshaftType::Normal => vanilla_blocks::OAK_LOG.default_state(),
            MineshaftType::Mesa => vanilla_blocks::DARK_OAK_LOG.default_state(),
        }
    }

    fn planks_state(mineshaft_type: MineshaftType) -> BlockStateId {
        match mineshaft_type {
            MineshaftType::Normal => vanilla_blocks::OAK_PLANKS.default_state(),
            MineshaftType::Mesa => vanilla_blocks::DARK_OAK_PLANKS.default_state(),
        }
    }

    fn fence_state(mineshaft_type: MineshaftType) -> BlockStateId {
        match mineshaft_type {
            MineshaftType::Normal => vanilla_blocks::OAK_FENCE.default_state(),
            MineshaftType::Mesa => vanilla_blocks::DARK_OAK_FENCE.default_state(),
        }
    }

    fn needs_shape_postprocessing(state: BlockStateId) -> bool {
        let block = state.get_block();
        block == &vanilla_blocks::WALL_TORCH
            || block == &vanilla_blocks::OAK_FENCE
            || block == &vanilla_blocks::DARK_OAK_FENCE
    }

    fn is_falling_block(state: BlockStateId) -> bool {
        let block = state.get_block();
        block == &vanilla_blocks::SAND
            || block == &vanilla_blocks::RED_SAND
            || block == &vanilla_blocks::GRAVEL
            || block == &vanilla_blocks::WHITE_CONCRETE_POWDER
            || block == &vanilla_blocks::ORANGE_CONCRETE_POWDER
            || block == &vanilla_blocks::MAGENTA_CONCRETE_POWDER
            || block == &vanilla_blocks::LIGHT_BLUE_CONCRETE_POWDER
            || block == &vanilla_blocks::YELLOW_CONCRETE_POWDER
            || block == &vanilla_blocks::LIME_CONCRETE_POWDER
            || block == &vanilla_blocks::PINK_CONCRETE_POWDER
            || block == &vanilla_blocks::GRAY_CONCRETE_POWDER
            || block == &vanilla_blocks::LIGHT_GRAY_CONCRETE_POWDER
            || block == &vanilla_blocks::CYAN_CONCRETE_POWDER
            || block == &vanilla_blocks::PURPLE_CONCRETE_POWDER
            || block == &vanilla_blocks::BLUE_CONCRETE_POWDER
            || block == &vanilla_blocks::BROWN_CONCRETE_POWDER
            || block == &vanilla_blocks::GREEN_CONCRETE_POWDER
            || block == &vanilla_blocks::RED_CONCRETE_POWDER
            || block == &vanilla_blocks::BLACK_CONCRETE_POWDER
            || block == &vanilla_blocks::ANVIL
            || block == &vanilla_blocks::CHIPPED_ANVIL
            || block == &vanilla_blocks::DAMAGED_ANVIL
            || block == &vanilla_blocks::DRAGON_EGG
    }
}
