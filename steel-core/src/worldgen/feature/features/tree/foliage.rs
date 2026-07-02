#![expect(
    clippy::match_same_arms,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "foliage dispatch keeps vanilla variant behavior explicit"
)]

use steel_registry::vanilla_block_tags::BlockTag;

use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::{FoliageAttachment, TreePlacement, abs_i32};

impl FeatureDecorationRunner {
    pub(super) fn tree_foliage_height(
        random: &mut WorldgenRandom,
        tree_height: i32,
        config: &TreeConfiguration,
    ) -> i32 {
        match &config.foliage_placer {
            FoliagePlacer::Blob(placer) => placer.height.sample(random),
            FoliagePlacer::Bush(placer) => placer.height.sample(random),
            FoliagePlacer::Fancy(placer) => placer.height.sample(random),
            FoliagePlacer::Pine(placer) => placer.height.sample(random),
            FoliagePlacer::Spruce(placer) => {
                (tree_height - placer.trunk_height.sample(random)).max(4)
            }
            FoliagePlacer::MegaPine(placer) => placer.crown_height.sample(random),
            FoliagePlacer::Acacia(_) => 0,
            FoliagePlacer::DarkOak(_) => 4,
            FoliagePlacer::Jungle(placer) => placer.height.sample(random),
            FoliagePlacer::RandomSpread(placer) => placer.foliage_height,
            FoliagePlacer::Cherry(placer) => placer.height.sample(random),
        }
    }

    pub(super) fn tree_foliage_radius(
        random: &mut WorldgenRandom,
        foliage_placer: &FoliagePlacer,
        trunk_height: i32,
    ) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.radius.sample(random),
            FoliagePlacer::Bush(placer) => placer.radius.sample(random),
            FoliagePlacer::Fancy(placer) => placer.radius.sample(random),
            FoliagePlacer::Pine(placer) => {
                placer.radius.sample(random) + random.next_i32_bounded((trunk_height + 1).max(1))
            }
            FoliagePlacer::Spruce(placer) => placer.radius.sample(random),
            FoliagePlacer::MegaPine(placer) => placer.radius.sample(random),
            FoliagePlacer::Acacia(placer) => placer.radius.sample(random),
            FoliagePlacer::DarkOak(placer) => placer.radius.sample(random),
            FoliagePlacer::Jungle(placer) => placer.radius.sample(random),
            FoliagePlacer::RandomSpread(placer) => placer.radius.sample(random),
            FoliagePlacer::Cherry(placer) => placer.radius.sample(random),
        }
    }

    pub(super) fn create_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
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
            FoliagePlacer::Fancy(_) => Self::create_fancy_tree_foliage(
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
            FoliagePlacer::DarkOak(_) => Self::create_dark_oak_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::Jungle(_) => Self::create_jungle_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::RandomSpread(placer) => Self::create_random_spread_tree_foliage(
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
            FoliagePlacer::Cherry(placer) => Self::create_cherry_tree_foliage(
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
        }
    }

    fn create_fancy_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        for y in (offset - foliage_height..=offset).rev() {
            let current_radius = if y != offset && y != offset - foliage_height {
                leaf_radius + 1
            } else {
                leaf_radius
            };
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

    fn create_jungle_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let leaf_height = if attachment.double_trunk {
            foliage_height
        } else {
            1 + random.next_i32_bounded(2)
        };

        for y in (offset - leaf_height..=offset).rev() {
            let current_radius = leaf_radius + attachment.radius_offset + 1 - y;
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

    fn create_random_spread_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        placer: &RandomSpreadFoliagePlacer,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        for _ in 0..placer.leaf_placement_attempts {
            let pos = attachment.pos.offset(
                random.next_i32_bounded(leaf_radius) - random.next_i32_bounded(leaf_radius),
                random.next_i32_bounded(foliage_height) - random.next_i32_bounded(foliage_height),
                random.next_i32_bounded(leaf_radius) - random.next_i32_bounded(leaf_radius),
            );
            let _ = Self::try_place_tree_leaf(region, registry, random, config, pos, placement);
        }
    }

    fn create_cherry_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        placer: &CherryFoliagePlacer,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let foliage_pos = attachment.pos.above_n(offset);
        let current_radius = leaf_radius + attachment.radius_offset - 1;
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            current_radius - 2,
            foliage_height - 3,
            attachment.double_trunk,
            placement,
        );
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            current_radius - 1,
            foliage_height - 4,
            attachment.double_trunk,
            placement,
        );

        for y in (0..=foliage_height - 5).rev() {
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                foliage_pos,
                current_radius,
                y,
                attachment.double_trunk,
                placement,
            );
        }

        Self::place_tree_leaves_row_with_hanging_leaves_below(
            region,
            registry,
            random,
            config,
            foliage_pos,
            current_radius,
            -1,
            attachment.double_trunk,
            placer.hanging_leaves_chance,
            placer.hanging_leaves_extension_chance,
            placement,
        );
        Self::place_tree_leaves_row_with_hanging_leaves_below(
            region,
            registry,
            random,
            config,
            foliage_pos,
            current_radius - 1,
            -2,
            attachment.double_trunk,
            placer.hanging_leaves_chance,
            placer.hanging_leaves_extension_chance,
            placement,
        );
    }

    fn create_dark_oak_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let pos = attachment.pos.above_n(offset);
        if attachment.double_trunk {
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                pos,
                leaf_radius + 2,
                -1,
                true,
                placement,
            );
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                pos,
                leaf_radius + 3,
                0,
                true,
                placement,
            );
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                pos,
                leaf_radius + 2,
                1,
                true,
                placement,
            );
            if random.next_bool() {
                Self::place_tree_leaves_row(
                    region,
                    registry,
                    random,
                    config,
                    pos,
                    leaf_radius,
                    2,
                    true,
                    placement,
                );
            }
        } else {
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                pos,
                leaf_radius + 2,
                -1,
                false,
                placement,
            );
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                pos,
                leaf_radius + 1,
                0,
                false,
                placement,
            );
        }
    }

    fn create_mega_pine_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
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
                + fast_floor(f64::from(y_offset) / f64::from(foliage_height) * 3.5);
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
        random: &mut WorldgenRandom,
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
        random: &mut WorldgenRandom,
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
        random: &mut WorldgenRandom,
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
        random: &mut WorldgenRandom,
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
        random: &mut WorldgenRandom,
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

    fn tree_foliage_offset(random: &mut WorldgenRandom, foliage_placer: &FoliagePlacer) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.offset.sample(random),
            FoliagePlacer::Bush(placer) => placer.offset.sample(random),
            FoliagePlacer::Fancy(placer) => placer.offset.sample(random),
            FoliagePlacer::Pine(placer) => placer.offset.sample(random),
            FoliagePlacer::Spruce(placer) => placer.offset.sample(random),
            FoliagePlacer::MegaPine(placer) => placer.offset.sample(random),
            FoliagePlacer::Acacia(placer) => placer.offset.sample(random),
            FoliagePlacer::DarkOak(placer) => placer.offset.sample(random),
            FoliagePlacer::Jungle(placer) => placer.offset.sample(random),
            FoliagePlacer::RandomSpread(placer) => placer.offset.sample(random),
            FoliagePlacer::Cherry(placer) => placer.offset.sample(random),
        }
    }

    fn place_tree_leaves_row(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        origin: BlockPos,
        current_radius: i32,
        y: i32,
        double_trunk: bool,
        placement: &mut TreePlacement,
    ) {
        let offset = i32::from(double_trunk);
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

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors vanilla foliage row helper"
    )]
    fn place_tree_leaves_row_with_hanging_leaves_below(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        origin: BlockPos,
        current_radius: i32,
        y: i32,
        double_trunk: bool,
        hanging_leaves_chance: f32,
        hanging_leaves_extension_chance: f32,
        placement: &mut TreePlacement,
    ) {
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            origin,
            current_radius,
            y,
            double_trunk,
            placement,
        );

        let offset = i32::from(double_trunk);
        let log_pos = origin.below();
        for along_edge in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            let to_edge = along_edge.rotate_y_clockwise();
            let offset_to_edge = if Self::direction_is_positive(to_edge) {
                current_radius + offset
            } else {
                current_radius
            };
            let mut pos = origin
                .offset(0, y - 1, 0)
                .relative_n(to_edge, offset_to_edge)
                .relative_n(along_edge, -current_radius);
            let mut offset_along_edge = -current_radius;

            while offset_along_edge < current_radius + offset {
                let leaves_above = placement.foliage.contains(pos.above());
                if leaves_above
                    && Self::try_place_hanging_leaf_extension(
                        region,
                        registry,
                        random,
                        config,
                        hanging_leaves_chance,
                        log_pos,
                        pos,
                        placement,
                    )
                {
                    let _ = Self::try_place_hanging_leaf_extension(
                        region,
                        registry,
                        random,
                        config,
                        hanging_leaves_extension_chance,
                        log_pos,
                        pos.below(),
                        placement,
                    );
                }

                offset_along_edge += 1;
                pos = pos.relative(along_edge);
            }
        }
    }

    fn try_place_hanging_leaf_extension(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        chance: f32,
        log_pos: BlockPos,
        pos: BlockPos,
        placement: &mut TreePlacement,
    ) -> bool {
        if Self::manhattan_distance(pos, log_pos) >= 7 || random.next_f32() > chance {
            return false;
        }

        Self::try_place_tree_leaf(region, registry, random, config, pos, placement)
    }

    const fn direction_is_positive(direction: Direction) -> bool {
        matches!(
            direction,
            Direction::Up | Direction::South | Direction::East
        )
    }

    fn tree_foliage_should_skip_location(
        random: &mut WorldgenRandom,
        foliage_placer: &FoliagePlacer,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        if matches!(foliage_placer, FoliagePlacer::DarkOak(_)) {
            return Self::dark_oak_foliage_should_skip_location(
                dx,
                y,
                dz,
                current_radius,
                double_trunk,
            );
        }

        let (dx, dz) = Self::foliage_signed_distances(dx, dz, double_trunk);
        match foliage_placer {
            FoliagePlacer::Blob(_) => {
                Self::blob_foliage_should_skip_location(random, dx, y, dz, current_radius)
            }
            FoliagePlacer::Bush(_) => {
                Self::bush_foliage_should_skip_location(random, dx, dz, current_radius)
            }
            FoliagePlacer::Fancy(_) => {
                Self::fancy_foliage_should_skip_location(dx, dz, current_radius)
            }
            FoliagePlacer::Pine(_) | FoliagePlacer::Spruce(_) => {
                Self::conifer_foliage_should_skip_location(dx, dz, current_radius)
            }
            FoliagePlacer::MegaPine(_) => {
                Self::mega_pine_foliage_should_skip_location(dx, dz, current_radius)
            }
            FoliagePlacer::Jungle(_) => {
                Self::mega_pine_foliage_should_skip_location(dx, dz, current_radius)
            }
            FoliagePlacer::RandomSpread(_) => false,
            FoliagePlacer::Acacia(_) => {
                Self::acacia_foliage_should_skip_location(dx, y, dz, current_radius)
            }
            FoliagePlacer::Cherry(placer) => {
                Self::cherry_foliage_should_skip_location(random, placer, dx, y, dz, current_radius)
            }
            FoliagePlacer::DarkOak(_) => unreachable!(),
        }
    }

    fn blob_foliage_should_skip_location(
        random: &mut WorldgenRandom,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        dx == current_radius && dz == current_radius && (random.next_i32_bounded(2) == 0 || y == 0)
    }

    fn bush_foliage_should_skip_location(
        random: &mut WorldgenRandom,
        dx: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        dx == current_radius && dz == current_radius && random.next_i32_bounded(2) == 0
    }

    fn fancy_foliage_should_skip_location(dx: i32, dz: i32, current_radius: i32) -> bool {
        let dx = dx as f32 + 0.5;
        let dz = dz as f32 + 0.5;
        dx * dx + dz * dz > (current_radius * current_radius) as f32
    }

    fn dark_oak_foliage_should_skip_location(
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        if y == 0
            && double_trunk
            && (dx == -current_radius || dx >= current_radius)
            && (dz == -current_radius || dz >= current_radius)
        {
            return true;
        }

        let (dx, dz) = Self::foliage_signed_distances(dx, dz, double_trunk);
        if y == -1 && !double_trunk {
            dx == current_radius && dz == current_radius
        } else if y == 1 {
            dx + dz > current_radius * 2 - 2
        } else {
            false
        }
    }

    fn cherry_foliage_should_skip_location(
        random: &mut WorldgenRandom,
        placer: &CherryFoliagePlacer,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        if y == -1
            && (dx == current_radius || dz == current_radius)
            && random.next_f32() < placer.wide_bottom_layer_hole_chance
        {
            return true;
        }

        let corner = dx == current_radius && dz == current_radius;
        let wide_layer = current_radius > 2;
        if wide_layer {
            corner
                || dx + dz > current_radius * 2 - 2 && random.next_f32() < placer.corner_hole_chance
        } else {
            corner && random.next_f32() < placer.corner_hole_chance
        }
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
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        pos: BlockPos,
        placement: &mut TreePlacement,
    ) -> bool {
        let current_state = region.block_state(pos);
        let is_persistent = current_state
            .try_get_value(&BlockStateProperties::PERSISTENT)
            .unwrap_or(false);
        let valid_tree_pos = current_state.is_air()
            || current_state
                .get_block()
                .has_tag(&BlockTag::REPLACEABLE_BY_TREES);
        if is_persistent || !valid_tree_pos {
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

    #[test]
    fn fancy_foliage_uses_shifted_circular_cutoff() {
        assert!(!FeatureDecorationRunner::fancy_foliage_should_skip_location(0, 0, 1));
        assert!(FeatureDecorationRunner::fancy_foliage_should_skip_location(
            1, 0, 1
        ));
        assert!(FeatureDecorationRunner::fancy_foliage_should_skip_location(
            0, 0, 0
        ));
    }

    #[test]
    fn dark_oak_double_trunk_skips_outer_corner_extensions_on_center_layer() {
        assert!(
            FeatureDecorationRunner::dark_oak_foliage_should_skip_location(-3, 0, -3, 3, true,)
        );
        assert!(FeatureDecorationRunner::dark_oak_foliage_should_skip_location(4, 0, 4, 3, true,));
        assert!(
            !FeatureDecorationRunner::dark_oak_foliage_should_skip_location(-2, 0, -3, 3, true,)
        );
    }

    #[test]
    fn dark_oak_side_crowns_skip_lower_outer_corners() {
        assert!(
            FeatureDecorationRunner::dark_oak_foliage_should_skip_location(2, -1, 2, 2, false,)
        );
        assert!(
            !FeatureDecorationRunner::dark_oak_foliage_should_skip_location(2, -1, 1, 2, false,)
        );
    }

    #[test]
    fn dark_oak_upper_layer_uses_diagonal_cutoff() {
        assert!(FeatureDecorationRunner::dark_oak_foliage_should_skip_location(3, 1, 2, 3, false,));
        assert!(
            !FeatureDecorationRunner::dark_oak_foliage_should_skip_location(2, 1, 2, 3, false,)
        );
    }
}
