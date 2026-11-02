//! Fast region fills for server tools and minigames (prison mines, arena resets, editors).
//!
//! Vanilla has no bulk write; `/fill` sets blocks one by one with neighbor updates. This
//! follows the approach of Paper-based fast fills (FAWE, prison mine plugins): each touched
//! chunk section is written under one lock without physics, block callbacks, neighbor or
//! shape updates, and without container drops. What stays consistent is everything a client
//! or later gameplay reads: section counters, block entities, heightmaps, light (queued only
//! where light properties change, then propagated by the normal per-tick light pass), points
//! of interest, saving, and client sync (one batched section update per section per tick).
//!
//! A fill refuses to start when any chunk it touches is not fully loaded. A chunk that
//! unloads after that check (it cannot while the caller holds the tick thread, but async
//! unloads are not excluded) is skipped and missing from [`FillReport::chunks`].
//! Mobs keep their current paths.

// TODO: invalidate mob navigation inside the filled box once Steel can do it per region
// instead of per block.

use std::fmt::{self, Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockPos, BlockStateId, ChunkPos, SectionPos};

use super::World;
use crate::chunk::full_chunk::bulk::ChunkFillOutput;

/// Region selected by a fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FillShape {
    /// Inclusive box.
    Cuboid {
        /// One corner.
        min: BlockPos,
        /// The opposite corner.
        max: BlockPos,
    },
    /// Blocks whose center is within `radius` of `center`'s center.
    Sphere {
        /// Center block.
        center: BlockPos,
        /// Radius in blocks.
        radius: f64,
    },
    /// Outer `thickness` blocks of a sphere.
    Shell {
        /// Center block.
        center: BlockPos,
        /// Outer radius in blocks.
        radius: f64,
        /// Wall thickness in blocks.
        thickness: f64,
    },
}

impl FillShape {
    /// One horizontal layer of a box at height `y`.
    #[must_use]
    pub const fn layer(min: BlockPos, max: BlockPos, y: i32) -> Self {
        Self::Cuboid {
            min: BlockPos::new(min.0.x, y, min.0.z),
            max: BlockPos::new(max.0.x, y, max.0.z),
        }
    }

    /// Inclusive bounding box.
    #[must_use]
    pub fn bounds(&self) -> (BlockPos, BlockPos) {
        match *self {
            Self::Cuboid { min, max } => (
                BlockPos::new(
                    min.0.x.min(max.0.x),
                    min.0.y.min(max.0.y),
                    min.0.z.min(max.0.z),
                ),
                BlockPos::new(
                    min.0.x.max(max.0.x),
                    min.0.y.max(max.0.y),
                    min.0.z.max(max.0.z),
                ),
            ),
            Self::Sphere { center, radius } | Self::Shell { center, radius, .. } => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "radii are far below the world coordinate range"
                )]
                let r = radius.max(0.0).ceil() as i32;
                (
                    BlockPos::new(center.0.x - r, center.0.y - r, center.0.z - r),
                    BlockPos::new(center.0.x + r, center.0.y + r, center.0.z + r),
                )
            }
        }
    }

    /// Whether `pos` belongs to the shape.
    #[must_use]
    pub fn contains(&self, pos: BlockPos) -> bool {
        let distance_sq = |center: BlockPos| {
            let dx = f64::from(pos.0.x - center.0.x);
            let dy = f64::from(pos.0.y - center.0.y);
            let dz = f64::from(pos.0.z - center.0.z);
            dx * dx + dy * dy + dz * dz
        };
        match *self {
            Self::Cuboid { .. } => {
                let (min, max) = self.bounds();
                (min.0.x..=max.0.x).contains(&pos.0.x)
                    && (min.0.y..=max.0.y).contains(&pos.0.y)
                    && (min.0.z..=max.0.z).contains(&pos.0.z)
            }
            Self::Sphere { center, radius } => distance_sq(center) <= radius * radius,
            Self::Shell {
                center,
                radius,
                thickness,
            } => {
                let inner = (radius - thickness).max(0.0);
                let d = distance_sq(center);
                d <= radius * radius && d > inner * inner
            }
        }
    }
}

/// States written by a fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillPattern {
    /// Every cell gets this state.
    Single(BlockStateId),
    /// Each cell picks a state with probability proportional to its weight.
    Weighted(Vec<(BlockStateId, u32)>),
}

/// Which existing cells a fill may replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillMode {
    /// Every cell in the shape.
    #[default]
    All,
    /// Only air cells (refill without overwriting).
    OnlyAir,
    /// Only non-air cells (clear or replace what is there).
    NonAir,
}

/// A fill request.
#[derive(Debug, Clone, PartialEq)]
pub struct FillRequest {
    /// Region.
    pub shape: FillShape,
    /// Optional inclusive box the shape is clipped to.
    pub clip: Option<(BlockPos, BlockPos)>,
    /// States to write.
    pub pattern: FillPattern,
    /// Replace mode.
    pub mode: FillMode,
}

/// Outcome of a fill.
#[derive(Debug, Clone, Default)]
pub struct FillReport {
    /// Cells whose state changed.
    pub changed: u64,
    /// Selected cells that already had the chosen state.
    pub unchanged: u64,
    /// Chunks touched.
    pub chunks: u32,
    /// Sections written.
    pub sections: u32,
    /// Replaced states with their counts (what was there before).
    pub replaced: FxHashMap<BlockStateId, u64>,
    /// Wall time of the write, excluding the deferred light propagation.
    pub elapsed: Duration,
}

/// Why a fill did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillError {
    /// The region misses the world's height range or the clip box.
    Empty,
    /// A touched chunk is not fully loaded.
    Unloaded(ChunkPos),
    /// The pattern has no states or only zero weights.
    EmptyPattern,
}

impl Display for FillError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the fill region is empty"),
            Self::Unloaded(pos) => write!(f, "chunk {}, {} is not loaded", pos.0.x, pos.0.y),
            Self::EmptyPattern => f.write_str("the fill pattern has no states"),
        }
    }
}

/// `SplitMix64`: one cheap, well-distributed draw per cell; fills need no vanilla determinism.
struct CellRandom(u64);

impl CellRandom {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Cumulative weights for [`FillPattern::Weighted`].
struct Picker {
    states: Vec<BlockStateId>,
    cumulative: Vec<u64>,
    random: CellRandom,
}

impl Picker {
    fn new(pattern: &FillPattern) -> Result<Self, FillError> {
        let entries: Vec<(BlockStateId, u32)> = match pattern {
            FillPattern::Single(state) => vec![(*state, 1)],
            FillPattern::Weighted(entries) => entries
                .iter()
                .copied()
                .filter(|(_, weight)| *weight > 0)
                .collect(),
        };
        if entries.is_empty() {
            return Err(FillError::EmptyPattern);
        }
        let mut total = 0;
        let cumulative = entries
            .iter()
            .map(|(_, weight)| {
                total += u64::from(*weight);
                total
            })
            .collect();
        Ok(Self {
            states: entries.into_iter().map(|(state, _)| state).collect(),
            cumulative,
            random: CellRandom(rand::random()),
        })
    }

    fn pick(&mut self) -> BlockStateId {
        if self.states.len() == 1 {
            return self.states[0];
        }
        let total = self.cumulative.last().copied().unwrap_or(1);
        let roll = self.random.next() % total;
        let index = self.cumulative.partition_point(|bound| *bound <= roll);
        self.states[index.min(self.states.len() - 1)]
    }
}

impl World {
    /// Fills a region with section-batched writes; see the [module docs](self).
    ///
    /// # Errors
    ///
    /// Returns [`FillError`] before writing anything when the region is empty, the pattern
    /// has no states, or a touched chunk is not fully loaded.
    pub fn fill_blocks(self: &Arc<Self>, request: &FillRequest) -> Result<FillReport, FillError> {
        let mut picker = Picker::new(&request.pattern)?;
        let mode = request.mode;
        self.fill_region(
            request.shape,
            request.clip,
            &mut |old| match mode {
                FillMode::All => true,
                FillMode::OnlyAir => old.is_air(),
                FillMode::NonAir => !old.is_air(),
            },
            &mut || picker.pick(),
        )
    }

    /// Replaces every block of the inclusive box whose state `replace` accepts with
    /// `state`, through the same section-batched path as [`Self::fill_blocks`].
    ///
    /// # Errors
    ///
    /// Returns [`FillError`] before writing anything when the box is empty or touches a
    /// chunk that is not fully loaded.
    pub fn replace_blocks(
        self: &Arc<Self>,
        min: BlockPos,
        max: BlockPos,
        replace: &dyn Fn(BlockStateId) -> bool,
        state: BlockStateId,
    ) -> Result<FillReport, FillError> {
        self.fill_region(
            FillShape::Cuboid { min, max },
            None,
            &mut |old| replace(old),
            &mut || state,
        )
    }

    fn fill_region(
        self: &Arc<Self>,
        shape: FillShape,
        clip: Option<(BlockPos, BlockPos)>,
        accepts: &mut dyn FnMut(BlockStateId) -> bool,
        pick: &mut dyn FnMut() -> BlockStateId,
    ) -> Result<FillReport, FillError> {
        let started = Instant::now();
        let (mut min, mut max) = shape.bounds();
        if let Some((a, b)) = clip {
            min = BlockPos::new(
                min.0.x.max(a.0.x.min(b.0.x)),
                min.0.y.max(a.0.y.min(b.0.y)),
                min.0.z.max(a.0.z.min(b.0.z)),
            );
            max = BlockPos::new(
                max.0.x.min(a.0.x.max(b.0.x)),
                max.0.y.min(a.0.y.max(b.0.y)),
                max.0.z.min(a.0.z.max(b.0.z)),
            );
        }
        min = BlockPos::new(min.0.x, min.0.y.max(self.get_min_y()), min.0.z);
        max = BlockPos::new(max.0.x, max.0.y.min(self.get_max_y()), max.0.z);
        if min.0.x > max.0.x || min.0.y > max.0.y || min.0.z > max.0.z {
            return Err(FillError::Empty);
        }

        let chunk_range = |a: i32, b: i32| {
            SectionPos::block_to_section_coord(a)..=SectionPos::block_to_section_coord(b)
        };
        let chunks: Vec<ChunkPos> = chunk_range(min.0.z, max.0.z)
            .flat_map(|cz| chunk_range(min.0.x, max.0.x).map(move |cx| ChunkPos::new(cx, cz)))
            .collect();
        if let Some(missing) = chunks
            .iter()
            .find(|pos| self.chunk_map.with_full_chunk(**pos, |_| ()).is_none())
        {
            return Err(FillError::Unloaded(*missing));
        }

        let mut decide = |pos: BlockPos, old: BlockStateId| {
            if !shape.contains(pos) || !accepts(old) {
                return None;
            }
            Some(pick())
        };

        let mut report = FillReport::default();
        for chunk_pos in chunks {
            let chunk_min_x = chunk_pos.0.x * 16;
            let chunk_min_z = chunk_pos.0.y * 16;
            let box_min =
                BlockPos::new(min.0.x.max(chunk_min_x), min.0.y, min.0.z.max(chunk_min_z));
            let box_max = BlockPos::new(
                max.0.x.min(chunk_min_x + 15),
                max.0.y,
                max.0.z.min(chunk_min_z + 15),
            );
            let mut output = ChunkFillOutput::default();
            if self
                .chunk_map
                .with_full_chunk(chunk_pos, |chunk| {
                    chunk.fill_box(box_min, box_max, &mut decide, &mut output);
                })
                .is_none()
            {
                // Unloaded between the check and the write: the remaining chunks still fill.
                continue;
            }
            report.chunks += 1;
            self.finish_chunk_fill(chunk_pos, output, &mut report);
        }
        report.elapsed = started.elapsed();
        Ok(report)
    }

    /// World-level follow-up for one filled chunk: client sync, light queue and POIs.
    fn finish_chunk_fill(
        &self,
        chunk_pos: ChunkPos,
        output: ChunkFillOutput,
        report: &mut FillReport,
    ) {
        for (section_index, positions) in &output.sections {
            report.sections += 1;
            report.changed += positions.len() as u64;
            self.chunk_map
                .section_blocks_changed(chunk_pos, *section_index, positions);
        }
        for pos in output.light {
            self.chunk_map.queue_light_change(pos, true, None);
        }
        for change in output.emptiness {
            let origin = BlockPos::new(
                change.section_pos.x() * 16,
                change.section_pos.y() * 16,
                change.section_pos.z() * 16,
            );
            self.chunk_map
                .queue_light_change(origin, false, Some(change));
        }
        if !output.poi.is_empty() {
            let mut poi = self.poi_storage.lock();
            for (pos, old, new) in output.poi {
                poi.on_block_state_change(pos, old, new);
            }
        }
        report.unchanged += output.unchanged;
        for (state, count) in output.replaced {
            *report.replaced.entry(state).or_default() += count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::chunk::heightmap::HeightmapType;
    use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};
    use steel_registry::{REGISTRY, blocks::BlockRef, init_vanilla_registry, vanilla_blocks};
    use steel_utils::types::UpdateFlags;

    fn setup(name: &'static str) -> TestWorld {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let fixture = fresh_test_world(name);
        for x in -1..=1 {
            for z in -1..=1 {
                insert_ready_full_chunk(&fixture.world, ChunkPos::new(x, z));
            }
        }
        fixture
    }

    fn state(block: BlockRef) -> BlockStateId {
        REGISTRY.blocks.get_base_state_id(block)
    }

    fn surface(world: &Arc<World>, x: i32, z: i32) -> i32 {
        let chunk = ChunkPos::new(x >> 4, z >> 4);
        world
            .chunk_map
            .with_full_chunk(chunk, |chunk| {
                chunk.get_height(
                    HeightmapType::WorldSurface,
                    (x & 15) as usize,
                    (z & 15) as usize,
                )
            })
            .unwrap_or(i32::MIN)
    }

    fn cuboid(pattern: FillPattern, mode: FillMode) -> FillRequest {
        FillRequest {
            shape: FillShape::Cuboid {
                min: BlockPos::new(-10, 200, -10),
                max: BlockPos::new(10, 203, 10),
            },
            clip: None,
            pattern,
            mode,
        }
    }

    #[test]
    fn cuboid_fill_writes_states_across_chunks_and_tracks_heightmaps() {
        let fixture = setup("bulk_fill_cuboid");
        let world = &fixture.world;
        let stone = state(&vanilla_blocks::STONE);
        let air = state(&vanilla_blocks::AIR);
        let before = surface(world, 0, 0);
        let Ok(report) = world.fill_blocks(&cuboid(FillPattern::Single(stone), FillMode::All))
        else {
            panic!("loaded region should fill");
        };
        assert_eq!(report.changed, 21 * 4 * 21);
        assert_eq!(report.chunks, 4);
        assert_eq!(report.replaced.get(&air), Some(&(21 * 4 * 21)));
        for pos in [
            BlockPos::new(-10, 200, -10),
            BlockPos::new(10, 203, 10),
            BlockPos::new(0, 201, 5),
        ] {
            assert_eq!(world.get_block_state(pos), stone);
        }
        assert_eq!(world.get_block_state(BlockPos::new(11, 201, 0)), air);
        assert_eq!(surface(world, 3, -4), 204);

        let Ok(cleared) = world.fill_blocks(&cuboid(FillPattern::Single(air), FillMode::NonAir))
        else {
            panic!("clearing should fill");
        };
        assert_eq!(cleared.replaced.get(&stone), Some(&(21 * 4 * 21)));
        assert_eq!(surface(world, 0, 0), before);
    }

    #[test]
    fn weighted_mix_histogram_counts_every_replaced_state() {
        let fixture = setup("bulk_fill_mix");
        let world = &fixture.world;
        let stone = state(&vanilla_blocks::STONE);
        let ore = state(&vanilla_blocks::DIAMOND_ORE);
        let air = state(&vanilla_blocks::AIR);
        let mix = FillPattern::Weighted(vec![(stone, 3), (ore, 1), (air, 0)]);
        assert!(world.fill_blocks(&cuboid(mix, FillMode::All)).is_ok());
        let Ok(mined) = world.fill_blocks(&cuboid(FillPattern::Single(air), FillMode::NonAir))
        else {
            panic!("mining should fill");
        };
        let stones = mined.replaced.get(&stone).copied().unwrap_or(0);
        let ores = mined.replaced.get(&ore).copied().unwrap_or(0);
        assert_eq!(stones + ores, 21 * 4 * 21);
        assert!(
            ores > 0 && stones > ores,
            "3:1 mix gave {stones} stone, {ores} ore"
        );
        assert_eq!(mined.replaced.len(), 2);
    }

    #[test]
    fn only_air_mode_keeps_existing_blocks() {
        let fixture = setup("bulk_fill_only_air");
        let world = &fixture.world;
        let stone = state(&vanilla_blocks::STONE);
        let gold = state(&vanilla_blocks::GOLD_BLOCK);
        let layer = FillRequest {
            shape: FillShape::layer(BlockPos::new(-10, 0, -10), BlockPos::new(10, 0, 10), 201),
            ..cuboid(FillPattern::Single(stone), FillMode::All)
        };
        assert!(world.fill_blocks(&layer).is_ok());
        let Ok(report) = world.fill_blocks(&cuboid(FillPattern::Single(gold), FillMode::OnlyAir))
        else {
            panic!("refill should run");
        };
        assert_eq!(report.changed, 21 * 3 * 21);
        assert_eq!(world.get_block_state(BlockPos::new(0, 201, 0)), stone);
        assert_eq!(world.get_block_state(BlockPos::new(0, 202, 0)), gold);
    }

    #[test]
    fn block_entities_follow_their_blocks() {
        let fixture = setup("bulk_fill_block_entities");
        let world = &fixture.world;
        // Comparators have a concrete block entity; chests are still unimplemented ones.
        let comparator = vanilla_blocks::COMPARATOR.default_state();
        let air = state(&vanilla_blocks::AIR);
        let pos = BlockPos::new(2, 210, 2);
        let request = |pattern| FillRequest {
            shape: FillShape::Cuboid { min: pos, max: pos },
            clip: None,
            pattern,
            mode: FillMode::All,
        };
        assert!(
            world
                .fill_blocks(&request(FillPattern::Single(comparator)))
                .is_ok()
        );
        assert!(world.get_block_entity(pos).is_some());
        assert!(
            world
                .fill_blocks(&request(FillPattern::Single(air)))
                .is_ok()
        );
        assert!(world.get_block_entity(pos).is_none());
    }

    #[test]
    fn unloaded_chunks_refuse_the_whole_fill() {
        let fixture = setup("bulk_fill_unloaded");
        let world = &fixture.world;
        let stone = state(&vanilla_blocks::STONE);
        let request = FillRequest {
            shape: FillShape::Cuboid {
                min: BlockPos::new(0, 200, 0),
                max: BlockPos::new(40, 200, 0),
            },
            clip: None,
            pattern: FillPattern::Single(stone),
            mode: FillMode::All,
        };
        assert_eq!(
            world.fill_blocks(&request).err(),
            Some(FillError::Unloaded(ChunkPos::new(2, 0)))
        );
        assert_ne!(world.get_block_state(BlockPos::new(0, 200, 0)), stone);
        assert_eq!(
            world
                .fill_blocks(&FillRequest {
                    pattern: FillPattern::Weighted(vec![(stone, 0)]),
                    ..request
                })
                .err(),
            Some(FillError::EmptyPattern)
        );
    }

    #[test]
    fn spheres_and_shells_select_by_distance() {
        let center = BlockPos::new(0, 100, 0);
        let sphere = FillShape::Sphere {
            center,
            radius: 3.0,
        };
        let shell = FillShape::Shell {
            center,
            radius: 3.0,
            thickness: 1.0,
        };
        assert!(sphere.contains(center));
        assert!(sphere.contains(BlockPos::new(3, 100, 0)));
        assert!(!sphere.contains(BlockPos::new(3, 101, 1)));
        assert!(!shell.contains(center));
        assert!(shell.contains(BlockPos::new(0, 103, 0)));
        assert!(!shell.contains(BlockPos::new(0, 101, 0)));
    }

    /// `cargo test --release -p steel-core --lib bulk_fill_benchmark -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark; run in release with --nocapture"]
    fn bulk_fill_benchmark() {
        let fixture = setup("bulk_fill_benchmark");
        let world = &fixture.world;
        for x in -4..=4 {
            for z in -4..=4 {
                if world
                    .chunk_map
                    .with_full_chunk(ChunkPos::new(x, z), |_| ())
                    .is_none()
                {
                    insert_ready_full_chunk(world, ChunkPos::new(x, z));
                }
            }
        }
        let stone = state(&vanilla_blocks::STONE);
        let ore = state(&vanilla_blocks::DIAMOND_ORE);
        let air = state(&vanilla_blocks::AIR);
        for side in [50, 100] {
            let min = BlockPos::new(-side / 2, 100, -side / 2);
            let max = BlockPos::new(side / 2 - 1, 100 + side - 1, side / 2 - 1);
            let blocks = f64::from(side * side * side);

            let started = Instant::now();
            for y in min.0.y..=max.0.y {
                for z in min.0.z..=max.0.z {
                    for x in min.0.x..=max.0.x {
                        world.set_block(BlockPos::new(x, y, z), stone, UpdateFlags::UPDATE_CLIENTS);
                    }
                }
            }
            let per_block = started.elapsed();

            let mix = FillRequest {
                shape: FillShape::Cuboid { min, max },
                clip: None,
                pattern: FillPattern::Weighted(vec![(ore, 1), (air, 1)]),
                mode: FillMode::All,
            };
            let Ok(report) = world.fill_blocks(&mix) else {
                panic!("benchmark region should be loaded");
            };
            eprintln!(
                "{blocks} blocks: per-block set_block {:.1} ms ({:.0} blocks/s), bulk fill {:.1} ms ({:.0} blocks/s, {} changed)",
                per_block.as_secs_f64() * 1000.0,
                blocks / per_block.as_secs_f64(),
                report.elapsed.as_secs_f64() * 1000.0,
                blocks / report.elapsed.as_secs_f64(),
                report.changed,
            );
        }
    }
}
