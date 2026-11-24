use std::{
    collections::BTreeMap,
    ptr,
    sync::{Arc, Weak},
};

use steel_protocol::packets::game::{CMapItemData, MapDecoration, MapPatch};
use steel_registry::{
    DyeColor, RegistryReference, data_components::vanilla_components::MapId, item_stack::ItemStack,
    map_decoration_type::MapDecorationTypeRef, vanilla_map_decoration_types,
};
use steel_utils::{BlockPos, Identifier};
use text_components::TextComponent;

use super::{DECORATION_EDGE, HALF_MAP_SIZE, MAP_SIZE};
use crate::player::Player;

const PLAYER_OFF_MAP_RANGE: f32 = 320.0;
const DECORATION_UNITS_PER_PIXEL: f32 = 2.0;
const ROTATION_STEPS: i32 = 16;
const DEGREES_PER_TURN: f64 = 360.0;
// Vanilla biases degrees before quantizing to sixteen directions.
const ROTATION_DEGREE_BIAS: f64 = 8.0;
const NETHER_ROTATION_TICKS_PER_STEP: i64 = 10;
const NETHER_ROTATION_QUADRATIC_MULTIPLIER: i32 = 34_187_121;
const NETHER_ROTATION_LINEAR_MULTIPLIER: i32 = 121;
const NETHER_ROTATION_NOISE_SHIFT: u32 = 15;
const DECORATION_UPDATE_INTERVAL: u32 = 5;

/// A named banner recorded on a map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapBanner {
    /// World position of the banner.
    pub pos: BlockPos,
    /// Base banner color.
    pub color: DyeColor,
    /// Optional custom name.
    pub name: Option<TextComponent>,
}

impl MapBanner {
    pub(super) fn key(&self) -> String {
        format!("banner-{},{},{}", self.pos.0.x, self.pos.0.y, self.pos.0.z)
    }

    pub(super) const fn decoration_type(&self) -> MapDecorationTypeRef {
        match self.color {
            DyeColor::White => &vanilla_map_decoration_types::BANNER_WHITE,
            DyeColor::Orange => &vanilla_map_decoration_types::BANNER_ORANGE,
            DyeColor::Magenta => &vanilla_map_decoration_types::BANNER_MAGENTA,
            DyeColor::LightBlue => &vanilla_map_decoration_types::BANNER_LIGHT_BLUE,
            DyeColor::Yellow => &vanilla_map_decoration_types::BANNER_YELLOW,
            DyeColor::Lime => &vanilla_map_decoration_types::BANNER_LIME,
            DyeColor::Pink => &vanilla_map_decoration_types::BANNER_PINK,
            DyeColor::Gray => &vanilla_map_decoration_types::BANNER_GRAY,
            DyeColor::LightGray => &vanilla_map_decoration_types::BANNER_LIGHT_GRAY,
            DyeColor::Cyan => &vanilla_map_decoration_types::BANNER_CYAN,
            DyeColor::Purple => &vanilla_map_decoration_types::BANNER_PURPLE,
            DyeColor::Blue => &vanilla_map_decoration_types::BANNER_BLUE,
            DyeColor::Brown => &vanilla_map_decoration_types::BANNER_BROWN,
            DyeColor::Green => &vanilla_map_decoration_types::BANNER_GREEN,
            DyeColor::Red => &vanilla_map_decoration_types::BANNER_RED,
            DyeColor::Black => &vanilla_map_decoration_types::BANNER_BLACK,
        }
    }
}

/// An item frame retained in the map's saved markers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFrame {
    /// Attachment position.
    pub pos: BlockPos,
    /// Facing rotation in degrees.
    pub rotation: i32,
    /// Runtime entity ID used to identify its decoration.
    pub entity_id: i32,
}

/// Terrain pixels and marker state for one saved map.
pub struct MapItemSavedData {
    /// World-space center X coordinate.
    pub center_x: i32,
    /// World-space center Z coordinate.
    pub center_z: i32,
    /// World in which terrain is sampled.
    pub dimension: Identifier,
    /// Each pixel spans `1 << scale` blocks.
    pub scale: i8,
    /// Whether terrain sampling has been disabled by locking the map.
    pub locked: bool,
    pub(super) tracking_position: bool,
    pub(super) unlimited_tracking: bool,
    pub(super) colors: Box<[u8; MAP_SIZE * MAP_SIZE]>,
    pub(super) banners: BTreeMap<[i32; 3], MapBanner>,
    pub(super) frames: BTreeMap<[i32; 3], MapFrame>,
    pub(super) decorations: Vec<(String, MapDecoration)>,
    pub(super) carried_by: Vec<HoldingPlayer>,
    pub(super) tracked_decoration_count: usize,
    pub(super) revision: u64,
    pub(super) saved_revision: u64,
}

pub(super) struct HoldingPlayer {
    pub(super) player: Weak<Player>,
    pub(super) name: String,
    pub(super) dirty: Option<DirtyRectangle>,
    pub(super) dirty_decorations: bool,
    pub(super) tick: u32,
    pub(super) step: i32,
    pub(super) updating: bool,
}

pub(super) struct DirtyRectangle {
    min_x: u8,
    min_y: u8,
    max_x: u8,
    max_y: u8,
}

impl MapItemSavedData {
    /// Largest supported map zoom level.
    pub const MAX_SCALE: i8 = 4;
    /// Tracked-marker limit checked before adding banners or framed maps.
    pub const TRACKED_DECORATION_LIMIT: usize = 256;

    /// Tests the marker limit used by frame insertion and banner tracking.
    #[must_use]
    pub const fn is_tracked_count_over_limit(&self, limit: usize) -> bool {
        self.tracked_decoration_count > limit
    }
    /// Stores an exploration target on the stack, ready to become a live marker
    /// when the map is carried or displayed in a frame.
    pub fn add_target_decoration(
        stack: &mut ItemStack,
        pos: BlockPos,
        key: String,
        decoration_type: MapDecorationTypeRef,
    ) {
        use steel_registry::data_components::vanilla_components::{
            MAP_DECORATIONS, MapDecorationEntry, MapDecorations,
        };
        let entry = MapDecorationEntry::new(
            RegistryReference::new(decoration_type),
            f64::from(pos.0.x),
            f64::from(pos.0.z),
            180.0,
        );
        let decorations = stack
            .get(MAP_DECORATIONS)
            .unwrap_or(&MapDecorations::EMPTY)
            .with_decoration(key, entry);
        stack.set(MAP_DECORATIONS, decorations);
    }

    pub(super) fn register_holder(&mut self, player: &Arc<Player>) -> usize {
        if let Some(index) = self
            .carried_by
            .iter()
            .position(|holder| holder.player.ptr_eq(&Arc::downgrade(player)))
        {
            return index;
        }
        self.carried_by.push(HoldingPlayer {
            player: Arc::downgrade(player),
            name: player.gameprofile.name().to_owned(),
            dirty: Some(DirtyRectangle {
                min_x: 0,
                min_y: 0,
                max_x: (MAP_SIZE - 1) as u8,
                max_y: (MAP_SIZE - 1) as u8,
            }),
            dirty_decorations: true,
            tick: 0,
            step: 0,
            updating: false,
        });
        self.carried_by.len() - 1
    }

    /// Aligns the map center to vanilla's scale-dependent grid.
    #[must_use]
    pub fn create_fresh(
        origin_x: f64,
        origin_z: f64,
        scale: i8,
        tracking_position: bool,
        unlimited_tracking: bool,
        dimension: Identifier,
    ) -> Self {
        let size = (MAP_SIZE as i32).wrapping_mul(1_i32.wrapping_shl(scale as u32));
        let center = |origin: f64| {
            let value = (origin + f64::from(HALF_MAP_SIZE)) / f64::from(size);
            let truncated = value as i32;
            let area = if value < f64::from(truncated) {
                truncated.wrapping_sub(1)
            } else {
                truncated
            };
            area.wrapping_mul(size)
                .wrapping_add(size / 2)
                .wrapping_sub(HALF_MAP_SIZE)
        };
        Self {
            center_x: center(origin_x),
            center_z: center(origin_z),
            dimension,
            scale,
            locked: false,
            tracking_position,
            unlimited_tracking,
            colors: Box::new([0; MAP_SIZE * MAP_SIZE]),
            banners: BTreeMap::new(),
            frames: BTreeMap::new(),
            decorations: Vec::new(),
            carried_by: Vec::new(),
            tracked_decoration_count: 0,
            revision: 1,
            saved_revision: 0,
        }
    }

    /// Reads packed colors in row-major order.
    #[must_use]
    pub fn colors(&self) -> &[u8; MAP_SIZE * MAP_SIZE] {
        &self.colors
    }

    /// Updates one pixel and expands every holder's pending rectangle.
    pub fn update_color(&mut self, x: u8, y: u8, color: u8) -> bool {
        let index = usize::from(x) + usize::from(y) * MAP_SIZE;
        if self.colors[index] == color {
            return false;
        }
        self.set_color(x, y, color);
        true
    }

    /// Sets a packed pixel, retaining vanilla's unconditional dirty notification.
    ///
    /// # Panics
    /// Panics when either coordinate is outside the 128 by 128 image.
    pub fn set_color(&mut self, x: u8, y: u8, color: u8) {
        assert!(
            usize::from(x) < MAP_SIZE && usize::from(y) < MAP_SIZE,
            "map pixel is outside the image"
        );
        self.colors[usize::from(x) + usize::from(y) * MAP_SIZE] = color;
        self.mark_dirty();
        for holder in &mut self.carried_by {
            match &mut holder.dirty {
                Some(rect) => {
                    rect.min_x = rect.min_x.min(x);
                    rect.min_y = rect.min_y.min(y);
                    rect.max_x = rect.max_x.max(x);
                    rect.max_y = rect.max_y.max(y);
                }
                None => {
                    holder.dirty = Some(DirtyRectangle {
                        min_x: x,
                        min_y: y,
                        max_x: x,
                        max_y: y,
                    });
                }
            }
        }
    }

    pub(super) const fn mark_dirty(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn decorations_dirty(&mut self) {
        for holder in &mut self.carried_by {
            holder.dirty_decorations = true;
        }
    }

    pub(super) fn remove_decoration(&mut self, key: &str) {
        if let Some(index) = self
            .decorations
            .iter()
            .position(|(candidate, _)| candidate == key)
        {
            let (_, previous) = self.decorations.remove(index);
            if previous.decoration_type.value().track_count {
                self.tracked_decoration_count -= 1;
            }
        }
        self.decorations_dirty();
    }

    /// Adds or moves a marker, preserving its original insertion position.
    #[expect(
        clippy::too_many_arguments,
        reason = "marker projection receives the position, rotation, dimension time, and label together"
    )]
    pub fn add_decoration(
        &mut self,
        decoration_type: MapDecorationTypeRef,
        key: String,
        x: f64,
        z: f64,
        rotation: f64,
        nether_time: Option<i64>,
        name: Option<TextComponent>,
    ) {
        let scale = 1_i32.wrapping_shl(self.scale as u32) as f32;
        let dx = (x - f64::from(self.center_x)) as f32 / scale;
        let dz = (z - f64::from(self.center_z)) as f32 / scale;
        let inside = (-DECORATION_EDGE..=DECORATION_EDGE).contains(&dx)
            && (-DECORATION_EDGE..=DECORATION_EDGE).contains(&dz);
        let decoration_type =
            if decoration_type.key == vanilla_map_decoration_types::PLAYER.key && !inside {
                if dx.abs() < PLAYER_OFF_MAP_RANGE && dz.abs() < PLAYER_OFF_MAP_RANGE {
                    &vanilla_map_decoration_types::PLAYER_OFF_MAP
                } else if self.unlimited_tracking {
                    &vanilla_map_decoration_types::PLAYER_OFF_LIMITS
                } else {
                    self.remove_decoration(&key);
                    return;
                }
            } else if !inside && !self.unlimited_tracking {
                self.remove_decoration(&key);
                return;
            } else {
                decoration_type
            };
        let coordinate = |delta: f32| {
            if delta <= -DECORATION_EDGE {
                i8::MIN
            } else if delta >= DECORATION_EDGE {
                i8::MAX
            } else {
                (delta * DECORATION_UNITS_PER_PIXEL + 0.5) as i32 as i8
            }
        };
        let rotation = if let Some(time) = nether_time {
            let s = (time / NETHER_ROTATION_TICKS_PER_STEP) as i32;
            (s.wrapping_mul(s)
                .wrapping_mul(NETHER_ROTATION_QUADRATIC_MULTIPLIER)
                .wrapping_add(s.wrapping_mul(NETHER_ROTATION_LINEAR_MULTIPLIER))
                >> NETHER_ROTATION_NOISE_SHIFT)
                & (ROTATION_STEPS - 1)
        } else {
            ((if rotation < 0.0 {
                rotation - ROTATION_DEGREE_BIAS
            } else {
                rotation + ROTATION_DEGREE_BIAS
            }) * f64::from(ROTATION_STEPS)
                / DEGREES_PER_TURN) as i32
        };
        let decoration = MapDecoration {
            decoration_type: RegistryReference::new(decoration_type),
            x: coordinate(dx),
            y: coordinate(dz),
            rotation: rotation as u8 & (ROTATION_STEPS - 1) as u8,
            name,
        };
        if let Some((_, previous)) = self
            .decorations
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            if *previous == decoration {
                return;
            }
            if previous.decoration_type.value().track_count {
                self.tracked_decoration_count -= 1;
            }
            *previous = decoration;
        } else {
            self.decorations.push((key, decoration));
        }
        if decoration_type.track_count {
            self.tracked_decoration_count += 1;
        }
        self.decorations_dirty();
    }

    /// Produces only changes for a previously registered carrier.
    pub fn update_packet(&mut self, id: MapId, player: &Player) -> Option<CMapItemData> {
        let holder = self.carried_by.iter_mut().find(|holder| {
            holder
                .player
                .upgrade()
                .is_some_and(|candidate| ptr::eq(candidate.as_ref(), player))
        })?;
        let patch = holder.dirty.take().map(|rect| {
            let width = rect.max_x + 1 - rect.min_x;
            let height = rect.max_y + 1 - rect.min_y;
            let mut colors = Vec::with_capacity(usize::from(width) * usize::from(height));
            for row in rect.min_y..=rect.max_y {
                let start = usize::from(row) * MAP_SIZE + usize::from(rect.min_x);
                colors.extend_from_slice(&self.colors[start..start + usize::from(width)]);
            }
            MapPatch {
                start_x: rect.min_x,
                start_y: rect.min_y,
                width,
                height,
                colors,
            }
        });
        let decorations = if holder.dirty_decorations {
            let tick = holder.tick;
            holder.tick = holder.tick.wrapping_add(1);
            if tick.is_multiple_of(DECORATION_UPDATE_INTERVAL) {
                holder.dirty_decorations = false;
                Some(
                    self.decorations
                        .iter()
                        .map(|(_, decoration)| decoration.clone())
                        .collect(),
                )
            } else {
                None
            }
        } else {
            None
        };
        if patch.is_none() && decorations.is_none() {
            return None;
        }
        Some(CMapItemData {
            map_id: id,
            scale: self.scale,
            locked: self.locked,
            decorations,
            color_patch: patch,
        })
    }
}
