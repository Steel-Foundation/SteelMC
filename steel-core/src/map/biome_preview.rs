//! Tick-sliced rendering of Vanilla's exploration-map biome preview.

use std::mem;

use steel_math::trig;
use steel_registry::vanilla_biome_tags::BiomeTag;
use steel_utils::BlockPos;

use crate::{map::MapItemSavedData, world::World};

const MAP_SIDE: usize = 128;
const MAP_PIXELS: usize = MAP_SIDE * MAP_SIDE;
const SAMPLE_ROWS_PER_POLL: usize = 16;
const RENDER_COLUMNS_PER_POLL: usize = 32;
const COLOR_ORANGE: u8 = 15;
const COLOR_BROWN: u8 = 26;
const BRIGHTNESS_LOW: u8 = 0;
const BRIGHTNESS_NORMAL: u8 = 1;
const BRIGHTNESS_HIGH: u8 = 2;
const BRIGHTNESS_LOWEST: u8 = 3;

/// Result of advancing a biome preview render.
pub(crate) enum BiomePreviewPoll {
    Pending,
    Ready(Vec<u8>),
    MissingBiome(BlockPos),
}

#[derive(Clone, Copy)]
enum PreviewPhase {
    Sampling { next_row: usize },
    Rendering { next_column: usize },
    Finished,
}

/// Resumable form of Vanilla `MapItem.renderBiomePreviewMap`.
pub(crate) struct BiomePreview {
    scale: i32,
    unscaled_start_x: i32,
    unscaled_start_z: i32,
    watery: Box<[bool; MAP_PIXELS]>,
    colors: Vec<u8>,
    phase: PreviewPhase,
}

impl BiomePreview {
    pub(crate) fn new(target: BlockPos, map_scale: i8) -> Self {
        let (center_x, center_z) = MapItemSavedData::fresh_center(target, map_scale);
        let scale = 1_i32.wrapping_shl(u32::from(map_scale as u8) & 31);
        Self {
            scale,
            unscaled_start_x: center_x / scale - 64,
            unscaled_start_z: center_z / scale - 64,
            watery: Box::new([false; MAP_PIXELS]),
            colors: vec![0; MAP_PIXELS],
            phase: PreviewPhase::Sampling { next_row: 0 },
        }
    }

    pub(crate) fn poll(&mut self, world: &World) -> BiomePreviewPoll {
        match self.phase {
            PreviewPhase::Sampling { next_row } => self.sample_rows(world, next_row),
            PreviewPhase::Rendering { next_column } => self.render_columns(next_column),
            PreviewPhase::Finished => BiomePreviewPoll::Ready(mem::take(&mut self.colors)),
        }
    }

    fn sample_rows(&mut self, world: &World, start: usize) -> BiomePreviewPoll {
        let end = (start + SAMPLE_ROWS_PER_POLL).min(MAP_SIDE);
        for row in start..end {
            for column in 0..MAP_SIDE {
                let pos = BlockPos::new(
                    (self.unscaled_start_x + column as i32).wrapping_mul(self.scale),
                    0,
                    (self.unscaled_start_z + row as i32).wrapping_mul(self.scale),
                );
                let Some(biome) = world.biome_at(pos) else {
                    return BiomePreviewPoll::MissingBiome(pos);
                };
                self.watery[row * MAP_SIDE + column] =
                    biome.has_tag(&BiomeTag::WATER_ON_MAP_OUTLINES);
            }
        }
        if end == MAP_SIDE {
            self.phase = PreviewPhase::Rendering { next_column: 1 };
        } else {
            self.phase = PreviewPhase::Sampling { next_row: end };
        }
        BiomePreviewPoll::Pending
    }

    fn render_columns(&mut self, start: usize) -> BiomePreviewPoll {
        let end = (start + RENDER_COLUMNS_PER_POLL).min(MAP_SIDE - 1);
        for mx in start..end {
            for mz in 1..(MAP_SIDE - 1) {
                let water_count = self.watery_neighbor_count(mx, mz);
                let (color, brightness) = if self.is_watery(mx, mz) {
                    Self::water_pixel(mx, mz, water_count)
                } else if water_count > 0 {
                    (
                        COLOR_BROWN,
                        if water_count > 3 {
                            BRIGHTNESS_NORMAL
                        } else {
                            BRIGHTNESS_LOWEST
                        },
                    )
                } else {
                    (0, BRIGHTNESS_LOWEST)
                };
                if color != 0 {
                    self.colors[mz * MAP_SIDE + mx] = packed_color(color, brightness);
                }
            }
        }
        if end == MAP_SIDE - 1 {
            self.phase = PreviewPhase::Finished;
            BiomePreviewPoll::Ready(mem::take(&mut self.colors))
        } else {
            self.phase = PreviewPhase::Rendering { next_column: end };
            BiomePreviewPoll::Pending
        }
    }

    fn water_pixel(mx: usize, mz: usize, water_count: u8) -> (u8, u8) {
        if water_count > 7 && mz.is_multiple_of(2) {
            let wave = trig::sin(mz as f64) * 7.0;
            let stripe = ((mx as i32 + wave as i32) / 8) % 5;
            let brightness = match stripe {
                0 | 4 => BRIGHTNESS_LOW,
                1 | 3 => BRIGHTNESS_NORMAL,
                2 => BRIGHTNESS_HIGH,
                _ => BRIGHTNESS_LOWEST,
            };
            (COLOR_ORANGE, brightness)
        } else if water_count > 7 {
            (0, BRIGHTNESS_LOWEST)
        } else if water_count > 5 {
            (COLOR_ORANGE, BRIGHTNESS_NORMAL)
        } else if water_count > 1 {
            (COLOR_ORANGE, BRIGHTNESS_LOW)
        } else {
            (COLOR_ORANGE, BRIGHTNESS_LOWEST)
        }
    }

    fn watery_neighbor_count(&self, x: usize, z: usize) -> u8 {
        let mut count = 0;
        for dx in -1_isize..=1 {
            for dz in -1_isize..=1 {
                if (dx != 0 || dz != 0)
                    && self.is_watery((x as isize + dx) as usize, (z as isize + dz) as usize)
                {
                    count += 1;
                }
            }
        }
        count
    }

    fn is_watery(&self, x: usize, z: usize) -> bool {
        self.watery[z * MAP_SIDE + x]
    }
}

const fn packed_color(color: u8, brightness: u8) -> u8 {
    (color << 2) | (brightness & 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_water_preview_uses_vanilla_even_row_stripes_and_blank_odd_rows() {
        let mut preview = BiomePreview::new(BlockPos::new(0, 0, 0), 1);
        preview.watery.fill(true);
        preview.phase = PreviewPhase::Rendering { next_column: 1 };

        let colors = render_all_columns(&mut preview);

        assert_eq!(colors[3 * MAP_SIDE + 64], 0);
        assert_ne!(colors[4 * MAP_SIDE + 64], 0);
        assert!(colors[..MAP_SIDE].iter().all(|&color| color == 0));
    }

    #[test]
    fn land_next_to_water_uses_brown_outline() {
        let mut preview = BiomePreview::new(BlockPos::new(0, 0, 0), 1);
        preview.watery[64 * MAP_SIDE + 64] = true;
        preview.phase = PreviewPhase::Rendering { next_column: 1 };

        let colors = render_all_columns(&mut preview);

        assert_eq!(
            colors[64 * MAP_SIDE + 63],
            packed_color(COLOR_BROWN, BRIGHTNESS_LOWEST)
        );
    }

    fn render_all_columns(preview: &mut BiomePreview) -> Vec<u8> {
        loop {
            let PreviewPhase::Rendering { next_column } = preview.phase else {
                panic!("preview should remain in its rendering phase");
            };
            match preview.render_columns(next_column) {
                BiomePreviewPoll::Pending => {}
                BiomePreviewPoll::Ready(colors) => return colors,
                BiomePreviewPoll::MissingBiome(_) => {
                    panic!("direct rendering should not query biomes")
                }
            }
        }
    }
}
