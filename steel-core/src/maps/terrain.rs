use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll, Waker},
};

use glam::DVec3;
use steel_registry::{
    blocks::block_state_ext::BlockStateExt,
    map_color::MapColor,
    vanilla_blocks,
    vanilla_map_colors::{self as colors, brightness},
};
use steel_utils::{BlockPos, ChunkPos, Direction, SectionPos, locks::SyncMutex};

use super::{HALF_MAP_SIZE, MAP_SIZE, MapItemSavedData, chunk_io::MapChunk};
use crate::{
    chunk::{full_chunk::FullChunkRef, heightmap::HeightmapType, status::ChunkStatus},
    entity::Entity,
    fluid::fluid_state_to_block,
    player::Player,
    server::jobs::{JobPoll, ServerJob, ServerJobContext},
    world::World,
};

const UPDATE_COLUMN_PERIOD: i32 = 16;
const BORDER_DITHER_WIDTH: i32 = 2;
const WATER_DEPTH_SHADING_SCALE: f64 = 0.1;
const WATER_DITHER_SHADING_SCALE: f64 = 0.2;
const WATER_HIGH_BRIGHTNESS_THRESHOLD: f64 = 0.5;
const WATER_LOW_BRIGHTNESS_THRESHOLD: f64 = 0.9;
const HEIGHT_SHADING_GAIN: f64 = 4.0;
const HEIGHT_SHADING_SCALE_BIAS: i32 = 4;
const HEIGHT_DITHER_SHADING_SCALE: f64 = 0.4;
const HEIGHT_BRIGHTNESS_THRESHOLD: f64 = 0.6;
const CEILING_NOISE_Z_MULTIPLIER: i32 = 231_871;
const CEILING_NOISE_QUADRATIC_MULTIPLIER: i32 = 31_287_121;
const CEILING_NOISE_LINEAR_MULTIPLIER: i32 = 11;
const CEILING_NOISE_SHIFT: u32 = 20;
const CEILING_SAMPLE_HEIGHT: f64 = 100.0;

struct UpdateLease {
    data: Arc<SyncMutex<MapItemSavedData>>,
    player: Weak<Player>,
}

impl Drop for UpdateLease {
    fn drop(&mut self) {
        let mut data = self.data.lock();
        if let Some(holder) = data
            .carried_by
            .iter_mut()
            .find(|holder| holder.player.ptr_eq(&self.player))
        {
            holder.updating = false;
        }
    }
}

struct MapUpdateJob {
    future: Pin<Box<dyn Future<Output = io::Result<()>> + Send>>,
}

impl MapUpdateJob {
    fn advance(&mut self) -> JobPoll {
        match self
            .future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Pending => JobPoll::Pending,
            Poll::Ready(result) => {
                if let Err(error) = result {
                    log::error!("Map terrain update failed: {error}");
                }
                JobPoll::Finished
            }
        }
    }
}

impl ServerJob for MapUpdateJob {
    fn poll(&mut self, _: &mut ServerJobContext) -> JobPoll {
        self.advance()
    }
    fn must_complete_before_shutdown(&self) -> bool {
        true
    }
}

struct RenderArea {
    center_x: i32,
    center_z: i32,
    scale: i32,
    step: i32,
    position: DVec3,
}

impl MapItemSavedData {
    /// Starts one terrain pass for this holder; unfinished passes resume through
    /// server jobs while chunk generation and other world ticks keep running.
    pub fn update(
        data: &Arc<SyncMutex<Self>>,
        world: &Arc<World>,
        player: &Arc<Player>,
    ) -> io::Result<()> {
        let area = {
            let mut data = data.lock();
            if data.locked || data.dimension != world.key {
                return Ok(());
            }
            let index = data.register_holder(player);
            let holder = &mut data.carried_by[index];
            if holder.updating {
                return Ok(());
            }
            holder.updating = true;
            holder.step = holder.step.wrapping_add(1);
            let step = holder.step;
            RenderArea {
                center_x: data.center_x,
                center_z: data.center_z,
                scale: 1_i32.wrapping_shl(data.scale as u32),
                step,
                position: player.position(),
            }
        };
        let lease = UpdateLease {
            data: Arc::clone(data),
            player: Arc::downgrade(player),
        };
        let world = Arc::clone(world);
        let storage = Arc::clone(&world.maps.read());
        let scheduler = storage.jobs.read().upgrade();
        let mut job = MapUpdateJob {
            future: Box::pin(async move {
                let lease = lease;
                let _admission = storage.update_gate.read().await;
                area.render(&world, &lease.data).await
            }),
        };
        if job.advance() == JobPoll::Finished {
            return Ok(());
        }
        let Some(scheduler) = scheduler else {
            return Err(io::Error::other(
                "pending map update has no server job scheduler",
            ));
        };
        scheduler.spawn(job);
        Ok(())
    }
}

impl RenderArea {
    async fn render(
        self,
        world: &Arc<World>,
        data: &Arc<SyncMutex<MapItemSavedData>>,
    ) -> io::Result<()> {
        let player_x = (self.position.x - f64::from(self.center_x)).floor() as i32 / self.scale
            + HALF_MAP_SIZE;
        let player_y = (self.position.z - f64::from(self.center_z)).floor() as i32 / self.scale
            + HALF_MAP_SIZE;
        let mut radius = MAP_SIZE as i32 / self.scale;
        if world.dimension_type.has_ceiling {
            radius /= 2;
        }
        let mut consecutive_changes = false;
        let mut cached: Option<MapChunk> = None;
        for x in player_x - radius + 1..player_x + radius {
            if x & (UPDATE_COLUMN_PERIOD - 1) != self.step & (UPDATE_COLUMN_PERIOD - 1)
                && !consecutive_changes
            {
                continue;
            }
            consecutive_changes = false;
            let mut previous_height = 0.0;
            for y in player_y - radius - 1..player_y + radius {
                if !(0..MAP_SIZE as i32).contains(&x) || !(-1..MAP_SIZE as i32).contains(&y) {
                    continue;
                }
                let distance = (x - player_x) * (x - player_x) + (y - player_y) * (y - player_y);
                let dither =
                    distance > (radius - BORDER_DITHER_WIDTH) * (radius - BORDER_DITHER_WIDTH);
                let min_x =
                    (self.center_x / self.scale + x - HALF_MAP_SIZE).wrapping_mul(self.scale);
                let min_z =
                    (self.center_z / self.scale + y - HALF_MAP_SIZE).wrapping_mul(self.scale);
                let pos = ChunkPos::from_block_pos(BlockPos::new(min_x, 0, min_z));
                if cached
                    .as_ref()
                    .is_none_or(|chunk| chunk.holder.get_pos() != pos)
                {
                    cached = Some(MapChunk::load(world, pos, ChunkStatus::Full).await?);
                }
                let Some(chunk) = cached
                    .as_ref()
                    .and_then(|chunk| chunk.holder.try_full_chunk())
                else {
                    return Err(io::Error::other("map terrain chunk lost Full readiness"));
                };
                let sample = self.sample(world, chunk, min_x, min_z, data);
                let brightness = if sample.color == colors::WATER {
                    let difference = f64::from(sample.water_depth) * WATER_DEPTH_SHADING_SCALE
                        + f64::from((x + y) & 1) * WATER_DITHER_SHADING_SCALE;
                    if difference < WATER_HIGH_BRIGHTNESS_THRESHOLD {
                        brightness::HIGH
                    } else if difference > WATER_LOW_BRIGHTNESS_THRESHOLD {
                        brightness::LOW
                    } else {
                        brightness::NORMAL
                    }
                } else {
                    let difference = (sample.height - previous_height) * HEIGHT_SHADING_GAIN
                        / f64::from(self.scale + HEIGHT_SHADING_SCALE_BIAS)
                        + (f64::from((x + y) & 1) - 0.5) * HEIGHT_DITHER_SHADING_SCALE;
                    if difference > HEIGHT_BRIGHTNESS_THRESHOLD {
                        brightness::HIGH
                    } else if difference < -HEIGHT_BRIGHTNESS_THRESHOLD {
                        brightness::LOW
                    } else {
                        brightness::NORMAL
                    }
                };
                previous_height = sample.height;
                if y >= 0 && distance < radius * radius && (!dither || (x + y) & 1 != 0) {
                    consecutive_changes |= data.lock().update_color(
                        x as u8,
                        y as u8,
                        sample.color.packed_id(brightness),
                    );
                }
            }
        }
        Ok(())
    }

    fn sample(
        &self,
        world: &World,
        chunk: FullChunkRef<'_>,
        min_x: i32,
        min_z: i32,
        data: &Arc<SyncMutex<MapItemSavedData>>,
    ) -> Sample {
        if world.dimension_type.has_ceiling {
            let noise = min_x.wrapping_add(min_z.wrapping_mul(CEILING_NOISE_Z_MULTIPLIER));
            let noise = noise
                .wrapping_mul(noise)
                .wrapping_mul(CEILING_NOISE_QUADRATIC_MULTIPLIER)
                .wrapping_add(noise.wrapping_mul(CEILING_NOISE_LINEAR_MULTIPLIER));
            let state = if noise >> CEILING_NOISE_SHIFT & 1 == 0 {
                vanilla_blocks::DIRT.default_state()
            } else {
                vanilla_blocks::STONE.default_state()
            };
            return Sample {
                color: state.get_map_color(),
                height: CEILING_SAMPLE_HEIGHT,
                water_depth: 0,
            };
        }
        let mut counts: Vec<(MapColor, i32)> = Vec::new();
        let mut height = 0.0;
        let mut water_depth = 0;
        for dx in 0..self.scale {
            for dz in 0..self.scale {
                let mut pos = BlockPos::new(min_x.wrapping_add(dx), 0, min_z.wrapping_add(dz));
                let relative = SectionPos::section_relative_pos(pos);
                let mut y = chunk.get_height(
                    HeightmapType::WorldSurface,
                    usize::from(relative.x()),
                    usize::from(relative.z()),
                );
                let mut state;
                if y <= chunk.min_y() {
                    state = vanilla_blocks::BEDROCK.default_state();
                } else {
                    loop {
                        y -= 1;
                        pos.0.y = y;
                        state = chunk.get_block_state(pos);
                        if state.get_map_color() != colors::NONE || y <= chunk.min_y() {
                            break;
                        }
                    }
                    if y > chunk.min_y() && !state.get_fluid_state().is_empty() {
                        let mut solid_y = y - 1;
                        loop {
                            let below =
                                chunk.get_block_state(BlockPos::new(pos.0.x, solid_y, pos.0.z));
                            solid_y -= 1;
                            water_depth += 1;
                            if solid_y <= chunk.min_y() || below.get_fluid_state().is_empty() {
                                break;
                            }
                        }
                        if !state.is_face_sturdy_at(pos, Direction::Up) {
                            state = fluid_state_to_block(state.get_fluid_state());
                        }
                    }
                }
                data.lock().check_banners(world, pos.0.x, pos.0.z);
                height += f64::from(y) / f64::from(self.scale * self.scale);
                let color = state.get_map_color();
                if let Some((_, count)) =
                    counts.iter_mut().find(|(candidate, _)| *candidate == color)
                {
                    *count += 1;
                } else {
                    counts.push((color, 1));
                }
            }
        }
        let color = counts
            .into_iter()
            .reduce(|first, next| if next.1 > first.1 { next } else { first })
            .map_or(colors::NONE, |entry| entry.0);
        Sample {
            color,
            height,
            water_depth: water_depth / (self.scale * self.scale),
        }
    }
}

struct Sample {
    color: MapColor,
    height: f64,
    water_depth: i32,
}

#[cfg(test)]
mod tests {
    #[test]
    fn pending_terrain_pass_survives_optional_cancellation_and_releases_holder_on_cancel() {
        use crate::{server::jobs::ServerJobQueue, test_support::TestPlayerBuilder};
        use tokio::runtime::Builder;

        init_globals();
        let fixture = fresh_test_world("map_update_jobs");
        let world = &fixture.world;
        let player = TestPlayerBuilder::new(Arc::clone(world), "MapUpdater", 1).build();
        let storage = Arc::clone(&world.maps.read());
        let jobs = Arc::new(ServerJobQueue::new());
        *storage.jobs.write() = Arc::downgrade(&jobs);
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let barrier = runtime.block_on(storage.update_gate.write());
        let data = Arc::new(SyncMutex::new(MapItemSavedData::create_fresh(
            0.0,
            0.0,
            0,
            true,
            false,
            world.key.clone(),
        )));

        MapItemSavedData::update(&data, world, &player).expect("queue terrain pass");
        assert!(data.lock().carried_by[0].updating);
        assert!(jobs.has_required_shutdown_work());
        jobs.cancel_optional();
        assert!(jobs.has_required_shutdown_work());
        // A second inventory tick must not queue another pass for this holder.
        MapItemSavedData::update(&data, world, &player).expect("existing terrain pass");
        assert_eq!(jobs.len(), 1);
        jobs.cancel_all();
        assert!(!data.lock().carried_by[0].updating);
        drop(barrier);
    }

    use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::{
        bootstrap::init_globals,
        test_support::{fresh_test_world, insert_ready_full_chunk},
    };

    #[test]
    fn terrain_sampling_skips_transparent_tops_and_keeps_first_color_when_area_counts_tie() {
        init_globals();
        let fixture = fresh_test_world("map_sample_tie");
        let world = &fixture.world;
        let holder = insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        for (x, z, block) in [
            (0, 0, &vanilla_blocks::STONE),
            (0, 1, &vanilla_blocks::DIRT),
            (1, 0, &vanilla_blocks::DIRT),
            (1, 1, &vanilla_blocks::STONE),
        ] {
            assert!(world.set_block(
                BlockPos::new(x, 70, z),
                block.default_state(),
                UpdateFlags::UPDATE_NONE
            ));
            assert!(world.set_block(
                BlockPos::new(x, 71, z),
                vanilla_blocks::GLASS.default_state(),
                UpdateFlags::UPDATE_NONE
            ));
        }
        let data = Arc::new(SyncMutex::new(MapItemSavedData::create_fresh(
            0.0,
            0.0,
            1,
            true,
            false,
            world.key.clone(),
        )));
        let area = RenderArea {
            center_x: 0,
            center_z: 0,
            scale: 2,
            step: 0,
            position: DVec3::ZERO,
        };
        let sample = area.sample(
            world,
            holder.try_full_chunk().expect("full chunk"),
            0,
            0,
            &data,
        );
        assert_eq!(
            sample.color,
            vanilla_blocks::STONE.default_state().get_map_color()
        );
        assert_eq!(sample.height, 70.0);
        assert_eq!(sample.water_depth, 0);
    }

    #[test]
    fn terrain_sampling_uses_fluid_color_for_waterlogged_partial_blocks_and_counts_the_bottom_block()
     {
        init_globals();
        let fixture = fresh_test_world("map_sample_water");
        let world = &fixture.world;
        let holder = insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(0, 63, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE
        ));
        for y in 64..70 {
            assert!(world.set_block(
                BlockPos::new(0, y, 0),
                vanilla_blocks::WATER.default_state(),
                UpdateFlags::UPDATE_NONE
            ));
        }
        let waterlogged: &BoolProperty = &BlockStateProperties::WATERLOGGED;
        assert!(
            world.set_block(
                BlockPos::new(0, 70, 0),
                vanilla_blocks::OAK_FENCE
                    .default_state()
                    .set_value(waterlogged, true),
                UpdateFlags::UPDATE_NONE
            )
        );
        let data = Arc::new(SyncMutex::new(MapItemSavedData::create_fresh(
            0.0,
            0.0,
            0,
            true,
            false,
            world.key.clone(),
        )));
        let area = RenderArea {
            center_x: 0,
            center_z: 0,
            scale: 1,
            step: 0,
            position: DVec3::ZERO,
        };
        let sample = area.sample(
            world,
            holder.try_full_chunk().expect("full chunk"),
            0,
            0,
            &data,
        );
        assert_eq!(sample.color, colors::WATER);
        assert_eq!(sample.height, 70.0);
        assert_eq!(sample.water_depth, 7);
    }
}
