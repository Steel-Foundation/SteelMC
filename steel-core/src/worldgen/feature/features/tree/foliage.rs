use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::{FoliageAttachment, TreePlacement, abs_i32};

impl FeatureDecorationRunner {
    pub(super) const fn tree_foliage_placer_supported(foliage_placer: &FoliagePlacer) -> bool {
        matches!(
            foliage_placer,
            FoliagePlacer::Blob(_)
                | FoliagePlacer::Bush(_)
                | FoliagePlacer::Pine(_)
                | FoliagePlacer::Spruce(_)
                | FoliagePlacer::MegaPine(_)
                | FoliagePlacer::Acacia(_)
        )
    }

    pub(super) fn tree_foliage_height(
        random: &mut Xoroshiro,
        tree_height: i32,
        config: &TreeConfiguration,
    ) -> i32 {
        match &config.foliage_placer {
            FoliagePlacer::Blob(placer) => placer.height.sample(random),
            FoliagePlacer::Bush(placer) => placer.height.sample(random),
            FoliagePlacer::Pine(placer) => placer.height.sample(random),
            FoliagePlacer::Spruce(placer) => {
                (tree_height - placer.trunk_height.sample(random)).max(4)
            }
            FoliagePlacer::MegaPine(placer) => placer.crown_height.sample(random),
            FoliagePlacer::Acacia(_) => 0,
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    pub(super) fn tree_foliage_radius(
        random: &mut Xoroshiro,
        foliage_placer: &FoliagePlacer,
        trunk_height: i32,
    ) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.radius.sample(random),
            FoliagePlacer::Bush(placer) => placer.radius.sample(random),
            FoliagePlacer::Pine(placer) => {
                placer.radius.sample(random) + random.next_i32_bounded((trunk_height + 1).max(1))
            }
            FoliagePlacer::Spruce(placer) => placer.radius.sample(random),
            FoliagePlacer::MegaPine(placer) => placer.radius.sample(random),
            FoliagePlacer::Acacia(placer) => placer.radius.sample(random),
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    pub(super) fn create_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        _tree_height: i32,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        match &config.foliage_placer {
            FoliagePlacer::Blob(placer) => Self::create_blob_tree_foliage(
                region,
                registry,
                random,
                config,
                placer,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::Bush(_) => Self::create_bush_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::Pine(_) => Self::create_pine_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::Spruce(_) => Self::create_spruce_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::MegaPine(_) => Self::create_mega_pine_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::Acacia(_) => Self::create_acacia_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    fn create_mega_pine_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let mut previous_radius = 0;
        let min_y = attachment.pos.y() - foliage_height + offset;
        let max_y = attachment.pos.y() + offset;

        for y in min_y..=max_y {
            let y_offset = attachment.pos.y() - y;
            let smooth_radius = leaf_radius
                + attachment.radius_offset
                + floor(y_offset as f64 / foliage_height as f64 * 3.5);
            let jagged_radius = if y_offset > 0 && smooth_radius == previous_radius && (y & 1) == 0
            {
                smooth_radius + 1
            } else {
                smooth_radius
            };
            let row_origin = BlockPos::new(attachment.pos.x(), y, attachment.pos.z());
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                row_origin,
                jagged_radius,
                0,
                attachment.double_trunk,
                placement,
            );
            previous_radius = smooth_radius;
        }
    }

    fn create_bush_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        for y in (offset - foliage_height..=offset).rev() {
            let current_radius = leaf_radius + attachment.radius_offset - 1 - y;
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                attachment.pos,
                current_radius,
                y,
                attachment.double_trunk,
                placement,
            );
        }
    }

    fn create_pine_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let mut current_radius = 0;
        for y in (offset - foliage_height..=offset).rev() {
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                attachment.pos,
                current_radius,
                y,
                attachment.double_trunk,
                placement,
            );
            if current_radius >= 1 && y == offset - foliage_height + 1 {
                current_radius -= 1;
            } else if current_radius < leaf_radius + attachment.radius_offset {
                current_radius += 1;
            }
        }
    }

    fn create_spruce_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let mut current_radius = random.next_i32_bounded(2);
        let mut max_radius = 1;
        let mut min_radius = 0;

        for y in (-foliage_height..=offset).rev() {
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                attachment.pos,
                current_radius,
                y,
                attachment.double_trunk,
                placement,
            );
            if current_radius >= max_radius {
                current_radius = min_radius;
                min_radius = 1;
                max_radius = (max_radius + 1).min(leaf_radius + attachment.radius_offset);
            } else {
                current_radius += 1;
            }
        }
    }

    fn create_blob_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        _placer: &BlobFoliagePlacer,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        for y in (offset - foliage_height..=offset).rev() {
            let current_radius = (leaf_radius + attachment.radius_offset - 1 - y / 2).max(0);
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                attachment.pos,
                current_radius,
                y,
                attachment.double_trunk,
                placement,
            );
        }
    }

    fn create_acacia_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let foliage_pos = attachment.pos.above_n(offset);
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            leaf_radius + attachment.radius_offset,
            -1 - foliage_height,
            attachment.double_trunk,
            placement,
        );
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            leaf_radius - 1,
            -foliage_height,
            attachment.double_trunk,
            placement,
        );
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            leaf_radius + attachment.radius_offset - 1,
            0,
            attachment.double_trunk,
            placement,
        );
    }

    fn tree_foliage_offset(random: &mut Xoroshiro, foliage_placer: &FoliagePlacer) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.offset.sample(random),
            FoliagePlacer::Bush(placer) => placer.offset.sample(random),
            FoliagePlacer::Pine(placer) => placer.offset.sample(random),
            FoliagePlacer::Spruce(placer) => placer.offset.sample(random),
            FoliagePlacer::MegaPine(placer) => placer.offset.sample(random),
            FoliagePlacer::Acacia(placer) => placer.offset.sample(random),
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    fn place_tree_leaves_row(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        origin: BlockPos,
        current_radius: i32,
        y: i32,
        double_trunk: bool,
        placement: &mut TreePlacement,
    ) {
        let offset = if double_trunk { 1 } else { 0 };
        for dx in -current_radius..=current_radius + offset {
            for dz in -current_radius..=current_radius + offset {
                if !Self::tree_foliage_should_skip_location(
                    random,
                    &config.foliage_placer,
                    dx,
                    y,
                    dz,
                    current_radius,
                    double_trunk,
                ) {
                    let pos = origin.offset(dx, y, dz);
                    let _ =
                        Self::try_place_tree_leaf(region, registry, random, config, pos, placement);
                }
            }
        }
    }

    fn tree_foliage_should_skip_location(
        random: &mut Xoroshiro,
        foliage_placer: &FoliagePlacer,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        let (dx, dz) = Self::foliage_signed_distances(dx, dz, double_trunk);
        match foliage_placer {
            FoliagePlacer::Blob(_) => {
                Self::blob_foliage_should_skip_location(random, dx, y, dz, current_radius)
            }
            FoliagePlacer::Bush(_) => {
                Self::bush_foliage_should_skip_location(random, dx, dz, current_radius)
            }
            FoliagePlacer::Pine(_) | FoliagePlacer::Spruce(_) => {
                Self::conifer_foliage_should_skip_location(dx, dz, current_radius)
            }
            FoliagePlacer::MegaPine(_) => {
                Self::mega_pine_foliage_should_skip_location(dx, dz, current_radius)
            }
            FoliagePlacer::Acacia(_) => {
                Self::acacia_foliage_should_skip_location(dx, y, dz, current_radius)
            }
            _ => {
                panic!(
                    "tree foliage placer requires runtime support before minecraft:tree can be registered"
                )
            }
        }
    }

    fn blob_foliage_should_skip_location(
        random: &mut Xoroshiro,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        dx == current_radius && dz == current_radius && (random.next_i32_bounded(2) == 0 || y == 0)
    }

    fn bush_foliage_should_skip_location(
        random: &mut Xoroshiro,
        dx: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        dx == current_radius && dz == current_radius && random.next_i32_bounded(2) == 0
    }

    const fn conifer_foliage_should_skip_location(dx: i32, dz: i32, current_radius: i32) -> bool {
        dx == current_radius && dz == current_radius && current_radius > 0
    }

    const fn mega_pine_foliage_should_skip_location(dx: i32, dz: i32, current_radius: i32) -> bool {
        dx + dz >= 7 || dx * dx + dz * dz > current_radius * current_radius
    }

    const fn acacia_foliage_should_skip_location(
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        if y == 0 {
            (dx > 1 || dz > 1) && dx != 0 && dz != 0
        } else {
            dx == current_radius && dz == current_radius && current_radius > 0
        }
    }

    fn foliage_signed_distances(dx: i32, dz: i32, double_trunk: bool) -> (i32, i32) {
        if double_trunk {
            (
                abs_i32(dx).min(abs_i32(dx - 1)),
                abs_i32(dz).min(abs_i32(dz - 1)),
            )
        } else {
            (abs_i32(dx), abs_i32(dz))
        }
    }

    fn try_place_tree_leaf(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        pos: BlockPos,
        placement: &mut TreePlacement,
    ) -> bool {
        let current_state = region.block_state(pos);
        let is_persistent = current_state
            .try_get_value(&BlockStateProperties::PERSISTENT)
            .unwrap_or(false);
        if is_persistent || !Self::tree_valid_pos(region, registry, pos) {
            return false;
        }

        let foliage_state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.foliage_provider,
            pos,
        );
        let foliage_state = Self::copy_waterlogged_from(region, pos, foliage_state);
        placement.set_foliage(region, pos, foliage_state);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acacia_top_layer_keeps_cross_and_inner_corners() {
        assert!(FeatureDecorationRunner::acacia_foliage_should_skip_location(2, 0, 2, 2));
        assert!(FeatureDecorationRunner::acacia_foliage_should_skip_location(1, 0, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(0, 0, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(1, 0, 1, 2));
    }

    #[test]
    fn acacia_lower_layers_skip_only_outer_corners() {
        assert!(FeatureDecorationRunner::acacia_foliage_should_skip_location(2, -1, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(1, -1, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(0, -1, 0, 0));
    }

    #[test]
    fn conifer_layers_skip_only_nonzero_outer_corners() {
        assert!(FeatureDecorationRunner::conifer_foliage_should_skip_location(2, 2, 2));
        assert!(!FeatureDecorationRunner::conifer_foliage_should_skip_location(1, 2, 2));
        assert!(!FeatureDecorationRunner::conifer_foliage_should_skip_location(0, 0, 0));
    }

    #[test]
    fn mega_pine_uses_circular_row_cutoff_with_vanilla_seven_limit() {
        assert!(FeatureDecorationRunner::mega_pine_foliage_should_skip_location(4, 3, 5));
        assert!(FeatureDecorationRunner::mega_pine_foliage_should_skip_location(4, 4, 5));
        assert!(!FeatureDecorationRunner::mega_pine_foliage_should_skip_location(3, 3, 5));
    }
}
