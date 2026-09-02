//! Vanilla filled-map terrain sampling for unlocked maps held in either hand.

use glam::DVec3;
use rustc_hash::FxHashSet;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, properties::Direction},
    map_color::{MapBrightness, MapColor},
    vanilla_blocks,
};
use steel_utils::{BlockPos, ChunkPos};

use crate::{
    chunk::heightmap::HeightmapType,
    fluid::fluid_state_to_block,
    world::{LevelReader, World},
};

use super::MapItemSavedData;

const MAP_SIDE: i32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MapDirtyBounds {
    pub(super) min_x: u8,
    pub(super) min_y: u8,
    pub(super) max_x: u8,
    pub(super) max_y: u8,
}

impl MapDirtyBounds {
    pub(super) const FULL: Self = Self {
        min_x: 0,
        min_y: 0,
        max_x: 127,
        max_y: 127,
    };

    const fn single(x: u8, y: u8) -> Self {
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    fn include(&mut self, x: u8, y: u8) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    pub(super) fn merge(&mut self, other: Self) {
        self.include(other.min_x, other.min_y);
        self.include(other.max_x, other.max_y);
    }
}

struct TerrainSample {
    average_height: f64,
    water_depth: i32,
    color: MapColor,
}

struct SamplingArea {
    scale: i32,
    player_image_x: i32,
    player_image_y: i32,
    radius: i32,
}

impl SamplingArea {
    fn new(world: &World, player_position: DVec3, map: &MapItemSavedData) -> Option<Self> {
        if map.locked || map.world != world.key {
            return None;
        }

        let scale = 1_i32 << map.scale.clamp(0, 4);
        let player_image_x =
            (player_position.x - f64::from(map.center_x)).floor() as i32 / scale + 64;
        let player_image_y =
            (player_position.z - f64::from(map.center_z)).floor() as i32 / scale + 64;
        let mut radius = MAP_SIDE / scale;
        if world.dimension_type.has_ceiling {
            radius /= 2;
        }

        Some(Self {
            scale,
            player_image_x,
            player_image_y,
            radius,
        })
    }
}

/// Returns every chunk Vanilla's map update loop may sample at the player's current position.
pub(super) fn sampling_chunks(
    world: &World,
    player_position: DVec3,
    map: &MapItemSavedData,
) -> Vec<ChunkPos> {
    let Some(area) = SamplingArea::new(world, player_position, map) else {
        return Vec::new();
    };
    let mut seen = FxHashSet::default();
    let mut chunks = Vec::new();
    for image_x in (area.player_image_x - area.radius + 1)..(area.player_image_x + area.radius) {
        if !(0..MAP_SIDE).contains(&image_x) {
            continue;
        }
        for image_y in (area.player_image_y - area.radius - 1)..(area.player_image_y + area.radius)
        {
            if !(-1..MAP_SIDE).contains(&image_y) {
                continue;
            }
            let area_min_x = (map.center_x / area.scale + image_x - 64) * area.scale;
            let area_min_z = (map.center_z / area.scale + image_y - 64) * area.scale;
            let chunk = ChunkPos::from_block_pos(BlockPos::new(area_min_x, 0, area_min_z));
            if seen.insert(chunk) {
                chunks.push(chunk);
            }
        }
    }
    chunks
}

/// Updates the Vanilla map stripe selected by `step`.
pub(super) fn update_map(
    world: &World,
    player_position: DVec3,
    step: i32,
    map: &mut MapItemSavedData,
) -> Option<MapDirtyBounds> {
    let area = SamplingArea::new(world, player_position, map)?;

    let mut dirty: Option<MapDirtyBounds> = None;
    let mut found_consecutive_changes = false;
    for image_x in (area.player_image_x - area.radius + 1)..(area.player_image_x + area.radius) {
        if image_x & 15 != step & 15 && !found_consecutive_changes {
            continue;
        }
        found_consecutive_changes = false;
        let mut previous_average_height = 0.0;

        for image_y in (area.player_image_y - area.radius - 1)..(area.player_image_y + area.radius)
        {
            if !(0..MAP_SIDE).contains(&image_x) || !(-1..MAP_SIDE).contains(&image_y) {
                continue;
            }

            let dx = image_x - area.player_image_x;
            let dy = image_y - area.player_image_y;
            let distance_squared = dx * dx + dy * dy;
            let dither_black = distance_squared > (area.radius - 2) * (area.radius - 2);
            let area_min_x = (map.center_x / area.scale + image_x - 64) * area.scale;
            let area_min_z = (map.center_z / area.scale + image_y - 64) * area.scale;
            let Some(sample) = terrain_sample(world, area_min_x, area_min_z, area.scale) else {
                continue;
            };
            let brightness = if sample.color == MapColor::WATER {
                water_brightness(sample.water_depth, image_x, image_y)
            } else {
                terrain_brightness(
                    sample.average_height,
                    previous_average_height,
                    area.scale,
                    image_x,
                    image_y,
                )
            };
            previous_average_height = sample.average_height;

            if image_y < 0
                || distance_squared >= area.radius * area.radius
                || (dither_black && ((image_x + image_y) & 1) == 0)
            {
                continue;
            }

            let index = (image_x + image_y * MAP_SIDE) as usize;
            let new_color = sample.color.packed_id(brightness);
            if map.colors[index] == new_color {
                continue;
            }
            map.colors[index] = new_color;
            found_consecutive_changes = true;
            let (x, y) = (image_x as u8, image_y as u8);
            if let Some(bounds) = &mut dirty {
                bounds.include(x, y);
            } else {
                dirty = Some(MapDirtyBounds::single(x, y));
            }
        }
    }
    dirty
}

fn terrain_sample(
    world: &World,
    area_min_x: i32,
    area_min_z: i32,
    scale: i32,
) -> Option<TerrainSample> {
    let chunk_pos = ChunkPos::from_block_pos(BlockPos::new(area_min_x, 0, area_min_z));
    world
        .chunk_map
        .with_full_chunk(chunk_pos, |chunk| {
            if world.dimension_type.has_ceiling {
                let mut noise = area_min_x.wrapping_add(area_min_z.wrapping_mul(231_871));
                noise = noise
                    .wrapping_mul(noise)
                    .wrapping_mul(31_287_121)
                    .wrapping_add(noise.wrapping_mul(11));
                return Some(TerrainSample {
                    average_height: 100.0,
                    water_depth: 0,
                    color: if noise.wrapping_shr(20) & 1 == 0 {
                        MapColor::DIRT
                    } else {
                        MapColor::STONE
                    },
                });
            }

            let min_y = world.get_min_y();
            let area = scale * scale;
            let mut average_height = 0.0;
            let mut water_depth = 0;
            let mut colors = Vec::<(MapColor, i32)>::new();
            for offset_x in 0..scale {
                for offset_z in 0..scale {
                    let x = area_min_x + offset_x;
                    let z = area_min_z + offset_z;
                    let mut column_y = chunk.get_height(
                        HeightmapType::WorldSurface,
                        (x & 15) as usize,
                        (z & 15) as usize,
                    );
                    let mut state;
                    if column_y <= min_y {
                        state = vanilla_blocks::BEDROCK.default_state();
                    } else {
                        let mut pos;
                        loop {
                            column_y -= 1;
                            pos = BlockPos::new(x, column_y, z);
                            state = chunk.get_block_state(pos);
                            if state.get_map_color() != MapColor::NONE || column_y <= min_y {
                                break;
                            }
                        }

                        if column_y > min_y && !state.get_fluid_state().is_empty() {
                            let mut solid_y = column_y - 1;
                            loop {
                                let below = chunk.get_block_state(BlockPos::new(x, solid_y, z));
                                solid_y -= 1;
                                water_depth += 1;
                                if solid_y <= min_y || below.get_fluid_state().is_empty() {
                                    break;
                                }
                            }
                            if !world.is_face_sturdy(state, pos, Direction::Up) {
                                state = fluid_state_to_block(state.get_fluid_state());
                            }
                        }
                    }

                    average_height += f64::from(column_y) / f64::from(area);
                    add_color(&mut colors, state.get_map_color());
                }
            }

            Some(TerrainSample {
                average_height,
                water_depth: water_depth / area,
                color: most_common_color(&colors),
            })
        })
        .flatten()
}

fn add_color(colors: &mut Vec<(MapColor, i32)>, color: MapColor) {
    if let Some((_, count)) = colors.iter_mut().find(|(existing, _)| *existing == color) {
        *count += 1;
    } else {
        colors.push((color, 1));
    }
}

fn most_common_color(colors: &[(MapColor, i32)]) -> MapColor {
    let mut best = (MapColor::NONE, 0);
    for &(color, count) in colors {
        if count > best.1 {
            best = (color, count);
        }
    }
    best.0
}

fn water_brightness(water_depth: i32, image_x: i32, image_y: i32) -> MapBrightness {
    let difference = f64::from(water_depth) * 0.1 + f64::from((image_x + image_y) & 1) * 0.2;
    if difference < 0.5 {
        MapBrightness::High
    } else if difference > 0.9 {
        MapBrightness::Low
    } else {
        MapBrightness::Normal
    }
}

fn terrain_brightness(
    average_height: f64,
    previous_average_height: f64,
    scale: i32,
    image_x: i32,
    image_y: i32,
) -> MapBrightness {
    let difference = (average_height - previous_average_height) * 4.0 / f64::from(scale + 4)
        + (f64::from((image_x + image_y) & 1) - 0.5) * 0.4;
    if difference > 0.6 {
        MapBrightness::High
    } else if difference < -0.6 {
        MapBrightness::Low
    } else {
        MapBrightness::Normal
    }
}
