use steel_utils::{BlockPos, Downcast};

use super::{DECORATION_EDGE, MapBanner, MapItemSavedData};
use crate::{block_entity::entities::BannerBlockEntity, nameable::Nameable, world::World};

impl MapBanner {
    /// Reads the actual banner block entity, including its custom name.
    pub fn from_world(world: &World, pos: BlockPos) -> Option<Self> {
        let entity = world.get_block_entity(pos)?;
        let banner = entity.downcast_ref::<BannerBlockEntity>()?;
        Some(Self {
            pos,
            color: banner.base_color(),
            name: banner.custom_name(),
        })
    }
}

impl MapItemSavedData {
    /// Adds a banner or removes an identical existing marker.
    pub fn toggle_banner(&mut self, world: &World, pos: BlockPos) -> bool {
        let scale = f64::from(1_i32.wrapping_shl(self.scale as u32));
        let center = pos.get_center();
        let dx = (center.x - f64::from(self.center_x)) / scale;
        let dz = (center.z - f64::from(self.center_z)) / scale;
        let edge = f64::from(DECORATION_EDGE);
        if !(-edge..=edge).contains(&dx) || !(-edge..=edge).contains(&dz) {
            return false;
        }
        let Some(banner) = MapBanner::from_world(world, pos) else {
            return false;
        };
        let coordinate = pos.0.to_array();
        if self.banners.get(&coordinate) == Some(&banner) {
            self.banners.remove(&coordinate);
            self.remove_decoration(&banner.key());
            self.mark_dirty();
            return true;
        }
        if self.is_tracked_count_over_limit(Self::TRACKED_DECORATION_LIMIT) {
            return false;
        }
        let nether_time = (self.dimension.path.as_ref() == "the_nether").then(|| world.game_time());
        self.add_decoration(
            banner.decoration_type(),
            banner.key(),
            center.x,
            center.z,
            180.0,
            nether_time,
            banner.name.clone(),
        );
        self.banners.insert(coordinate, banner);
        self.mark_dirty();
        true
    }

    pub(super) fn check_banners(&mut self, world: &World, x: i32, z: i32) {
        let stale = self
            .banners
            .iter()
            .filter(|(_, banner)| banner.pos.0.x == x && banner.pos.0.z == z)
            .filter(|(_, expected)| {
                MapBanner::from_world(world, expected.pos).as_ref() != Some(*expected)
            })
            .map(|(position, _)| *position)
            .collect::<Vec<_>>();
        for position in stale {
            if let Some(banner) = self.banners.remove(&position) {
                self.remove_decoration(&banner.key());
                self.mark_dirty();
            }
        }
    }
}
