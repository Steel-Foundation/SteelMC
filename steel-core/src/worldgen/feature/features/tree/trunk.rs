#![expect(
    clippy::too_many_arguments,
    reason = "trunk placement helpers mirror vanilla placement state"
)]

use std::f32::consts::TAU;
use std::f64::consts::PI;

use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::{FoliageAttachment, TreePlacement, abs_i32};

const FANCY_TRUNK_HEIGHT_SCALE: f64 = 0.618;
const FANCY_CLUSTER_DENSITY_MAGIC: f64 = 1.382;
const FANCY_BRANCH_SLOPE: f64 = 0.381;
const FANCY_BRANCH_LENGTH_MAGIC: f64 = 0.328;

impl FeatureDecorationRunner {
    pub(super) fn tree_height(random: &mut WorldgenRandom, placer: &TrunkPlacer) -> i32 {
        match placer {
            TrunkPlacer::Straight(base)
            | TrunkPlacer::Giant(base)
            | TrunkPlacer::Fancy(base)
            | TrunkPlacer::Forking(base)
            | TrunkPlacer::DarkOak(base)
            | TrunkPlacer::MegaJungle(base) => Self::sample_tree_height(
                random,
                base.base_height,
                base.height_rand_a,
                base.height_rand_b,
            ),
            TrunkPlacer::Bending(placer) => Self::sample_tree_height(
                random,
                placer.base_height,
                placer.height_rand_a,
                placer.height_rand_b,
            ),
            TrunkPlacer::UpwardsBranching(placer) => Self::sample_tree_height(
                random,
                placer.base_height,
                placer.height_rand_a,
                placer.height_rand_b,
            ),
            TrunkPlacer::Cherry(placer) => Self::sample_tree_height(
                random,
                placer.base_height,
                placer.height_rand_a,
                placer.height_rand_b,
            ),
        }
    }

    fn sample_tree_height(
        random: &mut WorldgenRandom,
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    ) -> i32 {
        base_height
            + random.next_i32_bounded(height_rand_a + 1)
            + random.next_i32_bounded(height_rand_b + 1)
    }

    pub(super) fn place_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        match &config.trunk_placer {
            TrunkPlacer::Straight(_) => Self::place_straight_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            TrunkPlacer::Forking(_) => Self::place_forking_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            TrunkPlacer::Giant(_) => Self::place_giant_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            TrunkPlacer::Fancy(_) => Self::place_fancy_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            TrunkPlacer::DarkOak(_) => Self::place_dark_oak_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            TrunkPlacer::MegaJungle(_) => Self::place_mega_jungle_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            TrunkPlacer::Bending(placer) => Self::place_bending_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placer,
                placement,
            ),
            TrunkPlacer::UpwardsBranching(placer) => Self::place_upwards_branching_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placer,
                placement,
            ),
            TrunkPlacer::Cherry(placer) => Self::place_cherry_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placer,
                placement,
            ),
        }
    }

    fn place_straight_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        Self::place_below_trunk_block(region, registry, random, origin.below(), config, placement);

        for y in 0..tree_height {
            let pos = origin.above_n(y);
            let _ = Self::place_tree_log(region, registry, random, pos, config, placement);
        }

        vec![FoliageAttachment {
            pos: origin.above_n(tree_height),
            radius_offset: 0,
            double_trunk: false,
        }]
    }

    fn place_forking_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        Self::place_below_trunk_block(region, registry, random, origin.below(), config, placement);

        let mut attachments = Vec::new();
        let lean_direction = Self::random_horizontal_direction(random);
        let lean_height = tree_height - random.next_i32_bounded(4) - 1;
        let mut lean_steps = 3 - random.next_i32_bounded(3);
        let mut trunk_x = origin.x();
        let mut trunk_z = origin.z();
        let mut foliage_y = None;

        for y_offset in 0..tree_height {
            let y = origin.y() + y_offset;
            if y_offset >= lean_height && lean_steps > 0 {
                let (dx, dz) = lean_direction.offset_xz();
                trunk_x += dx;
                trunk_z += dz;
                lean_steps -= 1;
            }

            let pos = BlockPos::new(trunk_x, y, trunk_z);
            if Self::place_tree_log(region, registry, random, pos, config, placement) {
                foliage_y = Some(y + 1);
            }
        }

        if let Some(y) = foliage_y {
            attachments.push(FoliageAttachment {
                pos: BlockPos::new(trunk_x, y, trunk_z),
                radius_offset: 1,
                double_trunk: false,
            });
        }

        trunk_x = origin.x();
        trunk_z = origin.z();
        let branch_direction = Self::random_horizontal_direction(random);
        if branch_direction != lean_direction {
            let mut branch_y_offset = lean_height - random.next_i32_bounded(2) - 1;
            let mut branch_steps = 1 + random.next_i32_bounded(3);
            foliage_y = None;

            while branch_y_offset < tree_height && branch_steps > 0 {
                if branch_y_offset >= 1 {
                    let y = origin.y() + branch_y_offset;
                    let (dx, dz) = branch_direction.offset_xz();
                    trunk_x += dx;
                    trunk_z += dz;
                    let pos = BlockPos::new(trunk_x, y, trunk_z);
                    if Self::place_tree_log(region, registry, random, pos, config, placement) {
                        foliage_y = Some(y + 1);
                    }
                }

                branch_y_offset += 1;
                branch_steps -= 1;
            }

            if let Some(y) = foliage_y {
                attachments.push(FoliageAttachment {
                    pos: BlockPos::new(trunk_x, y, trunk_z),
                    radius_offset: 0,
                    double_trunk: false,
                });
            }
        }

        attachments
    }

    fn place_giant_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        let below = origin.below();
        Self::place_below_trunk_block(region, registry, random, below, config, placement);
        Self::place_below_trunk_block(
            region,
            registry,
            random,
            below.relative(Direction::East),
            config,
            placement,
        );
        Self::place_below_trunk_block(
            region,
            registry,
            random,
            below.relative(Direction::South),
            config,
            placement,
        );
        Self::place_below_trunk_block(
            region,
            registry,
            random,
            below.offset(1, 0, 1),
            config,
            placement,
        );

        for y in 0..tree_height {
            let _ = Self::place_tree_log_if_free(
                region,
                registry,
                random,
                origin.above_n(y),
                config,
                placement,
            );
            if y < tree_height - 1 {
                let _ = Self::place_tree_log_if_free(
                    region,
                    registry,
                    random,
                    origin.offset(1, y, 0),
                    config,
                    placement,
                );
                let _ = Self::place_tree_log_if_free(
                    region,
                    registry,
                    random,
                    origin.offset(1, y, 1),
                    config,
                    placement,
                );
                let _ = Self::place_tree_log_if_free(
                    region,
                    registry,
                    random,
                    origin.offset(0, y, 1),
                    config,
                    placement,
                );
            }
        }

        vec![FoliageAttachment {
            pos: origin.above_n(tree_height),
            radius_offset: 0,
            double_trunk: true,
        }]
    }

    fn place_mega_jungle_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        let mut attachments = Self::place_giant_tree_trunk(
            region,
            registry,
            random,
            tree_height,
            origin,
            config,
            placement,
        );

        let mut branch_height = tree_height - 2 - random.next_i32_bounded(4);
        while branch_height > tree_height / 2 {
            let angle = random.next_f32() * TAU;
            let mut branch_x = 0;
            let mut branch_z = 0;

            for branch_step in 0..5 {
                branch_x = (1.5_f32 + angle.cos() * branch_step as f32) as i32;
                branch_z = (1.5_f32 + angle.sin() * branch_step as f32) as i32;
                let pos = origin.offset(branch_x, branch_height - 3 + branch_step / 2, branch_z);
                let _ = Self::place_tree_log(region, registry, random, pos, config, placement);
            }

            attachments.push(FoliageAttachment {
                pos: origin.offset(branch_x, branch_height, branch_z),
                radius_offset: -2,
                double_trunk: false,
            });
            branch_height -= 2 + random.next_i32_bounded(4);
        }

        attachments
    }

    fn place_bending_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placer: &BendingTrunkPlacer,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        let direction = Self::random_horizontal_direction(random);
        let log_height = tree_height - 1;
        let mut pos = origin;
        Self::place_below_trunk_block(region, registry, random, origin.below(), config, placement);
        let mut foliage_points = Vec::new();

        for y in 0..=log_height {
            if y + 1 >= log_height + random.next_i32_bounded(2) {
                pos = pos.relative(direction);
            }

            let _ = Self::place_tree_log(region, registry, random, pos, config, placement);

            if y >= placer.min_height_for_leaves {
                foliage_points.push(FoliageAttachment {
                    pos,
                    radius_offset: 0,
                    double_trunk: false,
                });
            }

            pos = pos.relative(Direction::Up);
        }

        let bend_length = placer.bend_length.sample(random);
        for _ in 0..=bend_length {
            let _ = Self::place_tree_log(region, registry, random, pos, config, placement);
            foliage_points.push(FoliageAttachment {
                pos,
                radius_offset: 0,
                double_trunk: false,
            });
            pos = pos.relative(direction);
        }

        foliage_points
    }

    fn place_upwards_branching_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placer: &UpwardsBranchingTrunkPlacer,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        let mut attachments = Vec::new();

        for height_pos in 0..tree_height {
            let current_height = origin.y() + height_pos;
            let log_pos = BlockPos::new(origin.x(), current_height, origin.z());
            if Self::place_tree_log_growing_through(
                region,
                registry,
                random,
                log_pos,
                &placer.can_grow_through,
                config,
                placement,
            ) && height_pos < tree_height - 1
                && random.next_f32() < placer.place_branch_per_log_probability
            {
                let branch_dir = Self::random_horizontal_direction(random);
                let branch_len = placer.extra_branch_length.sample(random);
                let branch_pos = 0.max(branch_len - placer.extra_branch_length.sample(random) - 1);
                let branch_steps = placer.extra_branch_steps.sample(random);
                Self::place_upwards_branching_tree_branch(
                    region,
                    registry,
                    random,
                    tree_height,
                    config,
                    placer,
                    &mut attachments,
                    log_pos,
                    current_height,
                    branch_dir,
                    branch_pos,
                    branch_steps,
                    placement,
                );
            }

            if height_pos == tree_height - 1 {
                attachments.push(FoliageAttachment {
                    pos: BlockPos::new(origin.x(), current_height + 1, origin.z()),
                    radius_offset: 0,
                    double_trunk: false,
                });
            }
        }

        attachments
    }

    #[expect(clippy::too_many_arguments, reason = "mirrors vanilla branch state")]
    fn place_upwards_branching_tree_branch(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        config: &TreeConfiguration,
        placer: &UpwardsBranchingTrunkPlacer,
        attachments: &mut Vec<FoliageAttachment>,
        log_pos: BlockPos,
        current_height: i32,
        branch_dir: Direction,
        branch_pos: i32,
        mut branch_steps: i32,
        placement: &mut TreePlacement,
    ) {
        let mut height_along_branch = current_height + branch_pos;
        let mut log_x = log_pos.x();
        let mut log_z = log_pos.z();
        let mut branch_placement_index = branch_pos;

        while branch_placement_index < tree_height && branch_steps > 0 {
            if branch_placement_index >= 1 {
                let placement_height = current_height + branch_placement_index;
                let (dx, dz) = branch_dir.offset_xz();
                log_x += dx;
                log_z += dz;
                height_along_branch = placement_height;
                let branch_log_pos = BlockPos::new(log_x, placement_height, log_z);
                if Self::place_tree_log_growing_through(
                    region,
                    registry,
                    random,
                    branch_log_pos,
                    &placer.can_grow_through,
                    config,
                    placement,
                ) {
                    height_along_branch = placement_height + 1;
                }

                attachments.push(FoliageAttachment {
                    pos: branch_log_pos,
                    radius_offset: 0,
                    double_trunk: false,
                });
            }

            branch_placement_index += 1;
            branch_steps -= 1;
        }

        if height_along_branch - current_height > 1 {
            let foliage_pos = BlockPos::new(log_x, height_along_branch, log_z);
            attachments.push(FoliageAttachment {
                pos: foliage_pos,
                radius_offset: 0,
                double_trunk: false,
            });
            attachments.push(FoliageAttachment {
                pos: foliage_pos.below_n(2),
                radius_offset: 0,
                double_trunk: false,
            });
        }
    }

    fn place_cherry_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placer: &CherryTrunkPlacer,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        Self::place_below_trunk_block(region, registry, random, origin.below(), config, placement);
        let first_branch_offset =
            0.max(tree_height - 1 + placer.branch_start_offset_from_top.sample(random));
        let second_branch_provider = placer
            .branch_start_offset_from_top
            .with_max_inclusive(placer.branch_start_offset_from_top.max_inclusive - 1);
        let mut second_branch_offset =
            0.max(tree_height - 1 + second_branch_provider.sample(random));
        if second_branch_offset >= first_branch_offset {
            second_branch_offset += 1;
        }

        let branch_count = placer.branch_count.sample(random);
        let has_middle_branch = branch_count == 3;
        let has_both_side_branches = branch_count >= 2;
        let trunk_height = if has_middle_branch {
            tree_height
        } else if has_both_side_branches {
            first_branch_offset.max(second_branch_offset) + 1
        } else {
            first_branch_offset + 1
        };

        for y in 0..trunk_height {
            let _ = Self::place_tree_log(
                region,
                registry,
                random,
                origin.above_n(y),
                config,
                placement,
            );
        }

        let mut attachments = Vec::new();
        if has_middle_branch {
            attachments.push(FoliageAttachment {
                pos: origin.above_n(trunk_height),
                radius_offset: 0,
                double_trunk: false,
            });
        }

        let tree_direction = Self::random_horizontal_direction(random);
        let sideways_axis = tree_direction.get_axis();
        attachments.push(Self::generate_cherry_tree_branch(
            region,
            registry,
            random,
            tree_height,
            origin,
            config,
            placer,
            tree_direction,
            first_branch_offset,
            first_branch_offset < trunk_height - 1,
            sideways_axis,
            placement,
        ));
        if has_both_side_branches {
            attachments.push(Self::generate_cherry_tree_branch(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placer,
                tree_direction.opposite(),
                second_branch_offset,
                second_branch_offset < trunk_height - 1,
                sideways_axis,
                placement,
            ));
        }

        attachments
    }

    #[expect(clippy::too_many_arguments, reason = "mirrors vanilla branch state")]
    fn generate_cherry_tree_branch(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placer: &CherryTrunkPlacer,
        branch_direction: Direction,
        offset_from_origin: i32,
        middle_continues_upwards: bool,
        sideways_axis: Axis,
        placement: &mut TreePlacement,
    ) -> FoliageAttachment {
        let mut log_pos = origin.above_n(offset_from_origin);
        let branch_end_y = tree_height - 1 + placer.branch_end_offset_from_top.sample(random);
        let extend_branch_away_from_trunk =
            middle_continues_upwards || branch_end_y < offset_from_origin;
        let distance_to_trunk = placer.branch_horizontal_length.sample(random)
            + i32::from(extend_branch_away_from_trunk);
        let branch_end_pos = origin
            .relative_n(branch_direction, distance_to_trunk)
            .above_n(branch_end_y);
        let steps_horizontally = if extend_branch_away_from_trunk { 2 } else { 1 };

        for _ in 0..steps_horizontally {
            log_pos = log_pos.relative(branch_direction);
            let _ = Self::place_tree_log_with_axis(
                region,
                registry,
                random,
                log_pos,
                sideways_axis,
                config,
                placement,
            );
        }

        let vertical_direction = if branch_end_pos.y() > log_pos.y() {
            Direction::Up
        } else {
            Direction::Down
        };

        loop {
            let distance = Self::manhattan_distance(log_pos, branch_end_pos);
            if distance == 0 {
                return FoliageAttachment {
                    pos: branch_end_pos.above(),
                    radius_offset: 0,
                    double_trunk: false,
                };
            }

            let vertical_distance = (branch_end_pos.y() - log_pos.y()).abs();
            let grow_vertically = random.next_f32() < vertical_distance as f32 / distance as f32;
            log_pos = if grow_vertically {
                log_pos.relative(vertical_direction)
            } else {
                log_pos.relative(branch_direction)
            };

            if grow_vertically {
                let _ = Self::place_tree_log(region, registry, random, log_pos, config, placement);
            } else {
                let _ = Self::place_tree_log_with_axis(
                    region,
                    registry,
                    random,
                    log_pos,
                    sideways_axis,
                    config,
                    placement,
                );
            }
        }
    }

    fn place_dark_oak_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        let mut attachments = Vec::new();
        let below = origin.below();
        Self::place_below_trunk_block(region, registry, random, below, config, placement);
        Self::place_below_trunk_block(
            region,
            registry,
            random,
            below.relative(Direction::East),
            config,
            placement,
        );
        Self::place_below_trunk_block(
            region,
            registry,
            random,
            below.relative(Direction::South),
            config,
            placement,
        );
        Self::place_below_trunk_block(
            region,
            registry,
            random,
            below.offset(1, 0, 1),
            config,
            placement,
        );

        let lean_direction = Self::random_horizontal_direction(random);
        let lean_height = tree_height - random.next_i32_bounded(4);
        let mut lean_steps = 2 - random.next_i32_bounded(3);
        let x = origin.x();
        let y = origin.y();
        let z = origin.z();
        let mut trunk_x = x;
        let mut trunk_z = z;
        let foliage_y = y + tree_height - 1;

        for y_offset in 0..tree_height {
            if y_offset >= lean_height && lean_steps > 0 {
                let (dx, dz) = lean_direction.offset_xz();
                trunk_x += dx;
                trunk_z += dz;
                lean_steps -= 1;
            }

            let pos = BlockPos::new(trunk_x, y + y_offset, trunk_z);
            if Self::tree_is_air_or_leaves(region, pos) {
                let _ = Self::place_tree_log(region, registry, random, pos, config, placement);
                let _ = Self::place_tree_log(
                    region,
                    registry,
                    random,
                    pos.relative(Direction::East),
                    config,
                    placement,
                );
                let _ = Self::place_tree_log(
                    region,
                    registry,
                    random,
                    pos.relative(Direction::South),
                    config,
                    placement,
                );
                let _ = Self::place_tree_log(
                    region,
                    registry,
                    random,
                    pos.offset(1, 0, 1),
                    config,
                    placement,
                );
            }
        }

        attachments.push(FoliageAttachment {
            pos: BlockPos::new(trunk_x, foliage_y, trunk_z),
            radius_offset: 0,
            double_trunk: true,
        });

        for ox in -1..=2 {
            for oz in -1..=2 {
                if (0..=1).contains(&ox) && (0..=1).contains(&oz) {
                    continue;
                }
                if random.next_i32_bounded(3) > 0 {
                    continue;
                }

                let branch_length = random.next_i32_bounded(3) + 2;
                for branch_y in 0..branch_length {
                    let pos = BlockPos::new(x + ox, foliage_y - branch_y - 1, z + oz);
                    let _ = Self::place_tree_log(region, registry, random, pos, config, placement);
                }

                attachments.push(FoliageAttachment {
                    pos: BlockPos::new(x + ox, foliage_y, z + oz),
                    radius_offset: 0,
                    double_trunk: false,
                });
            }
        }

        attachments
    }

    fn place_fancy_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        let height = tree_height + 2;
        let trunk_height = fast_floor(f64::from(height) * FANCY_TRUNK_HEIGHT_SCALE);
        Self::place_below_trunk_block(region, registry, random, origin.below(), config, placement);

        let clusters_per_y = 1.min(fast_floor(
            FANCY_CLUSTER_DENSITY_MAGIC + (f64::from(height) / 13.0).powi(2),
        ));
        let trunk_top = origin.y() + trunk_height;
        let mut relative_y = height - 5;
        let mut foliage_coords = vec![FancyFoliageCoords {
            attachment: FoliageAttachment {
                pos: origin.above_n(relative_y),
                radius_offset: 0,
                double_trunk: false,
            },
            branch_base: trunk_top,
        }];

        while relative_y >= 0 {
            let tree_shape = Self::fancy_tree_shape(height, relative_y);
            if tree_shape >= 0.0 {
                for _ in 0..clusters_per_y {
                    let radius = f64::from(tree_shape)
                        * (f64::from(random.next_f32()) + FANCY_BRANCH_LENGTH_MAGIC);
                    let angle = f64::from(random.next_f32() * 2.0_f32) * PI;
                    let x = radius * angle.sin() + 0.5;
                    let z = radius * angle.cos() + 0.5;
                    let check_start = origin.offset(fast_floor(x), relative_y - 1, fast_floor(z));
                    let check_end = check_start.above_n(5);
                    if Self::make_fancy_tree_limb(
                        region,
                        registry,
                        random,
                        check_start,
                        check_end,
                        false,
                        config,
                        placement,
                    ) {
                        let dx = origin.x() - check_start.x();
                        let dz = origin.z() - check_start.z();
                        let branch_height = f64::from(check_start.y())
                            - f64::from(dx * dx + dz * dz).sqrt() * FANCY_BRANCH_SLOPE;
                        let branch_top = if branch_height > f64::from(trunk_top) {
                            trunk_top
                        } else {
                            branch_height as i32
                        };
                        let check_branch_base = BlockPos::new(origin.x(), branch_top, origin.z());
                        if Self::make_fancy_tree_limb(
                            region,
                            registry,
                            random,
                            check_branch_base,
                            check_start,
                            false,
                            config,
                            placement,
                        ) {
                            foliage_coords.push(FancyFoliageCoords {
                                attachment: FoliageAttachment {
                                    pos: check_start,
                                    radius_offset: 0,
                                    double_trunk: false,
                                },
                                branch_base: check_branch_base.y(),
                            });
                        }
                    }
                }
            }
            relative_y -= 1;
        }

        Self::make_fancy_tree_limb(
            region,
            registry,
            random,
            origin,
            origin.above_n(trunk_height),
            true,
            config,
            placement,
        );
        Self::make_fancy_tree_branches(
            region,
            registry,
            random,
            height,
            origin,
            &foliage_coords,
            config,
            placement,
        );

        foliage_coords
            .into_iter()
            .filter(|coord| Self::trim_fancy_tree_branch(height, coord.branch_base - origin.y()))
            .map(|coord| coord.attachment)
            .collect()
    }

    fn make_fancy_tree_limb(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        start_pos: BlockPos,
        end_pos: BlockPos,
        do_place: bool,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        if !do_place && start_pos == end_pos {
            return true;
        }

        let delta = BlockPos::new(
            end_pos.x() - start_pos.x(),
            end_pos.y() - start_pos.y(),
            end_pos.z() - start_pos.z(),
        );
        let steps = Self::fancy_tree_limb_steps(delta);
        if steps == 0 {
            if do_place {
                let _ = Self::place_fancy_tree_log(
                    region,
                    registry,
                    random,
                    start_pos,
                    Axis::Y,
                    config,
                    placement,
                );
            }
            return true;
        }

        let dx = delta.x() as f32 / steps as f32;
        let dy = delta.y() as f32 / steps as f32;
        let dz = delta.z() as f32 / steps as f32;

        for step in 0..=steps {
            let step = step as f32;
            let pos = start_pos.offset(
                fast_floor(f64::from(0.5_f32 + step * dx)),
                fast_floor(f64::from(0.5_f32 + step * dy)),
                fast_floor(f64::from(0.5_f32 + step * dz)),
            );
            if do_place {
                let axis = Self::fancy_tree_log_axis(start_pos, pos);
                let _ = Self::place_fancy_tree_log(
                    region, registry, random, pos, axis, config, placement,
                );
            } else if !Self::tree_trunk_placer_is_free(region, pos, &config.trunk_placer) {
                return false;
            }
        }

        true
    }

    fn fancy_tree_limb_steps(pos: BlockPos) -> i32 {
        let abs_x = abs_i32(pos.x());
        let abs_y = abs_i32(pos.y());
        let abs_z = abs_i32(pos.z());
        abs_x.max(abs_y).max(abs_z)
    }

    fn fancy_tree_log_axis(start_pos: BlockPos, block_pos: BlockPos) -> Axis {
        let xdiff = abs_i32(block_pos.x() - start_pos.x());
        let zdiff = abs_i32(block_pos.z() - start_pos.z());
        let maxdiff = xdiff.max(zdiff);
        if maxdiff == 0 {
            Axis::Y
        } else if xdiff == maxdiff {
            Axis::X
        } else {
            Axis::Z
        }
    }

    fn trim_fancy_tree_branch(height: i32, local_y: i32) -> bool {
        f64::from(local_y) >= f64::from(height) * 0.2
    }

    fn make_fancy_tree_branches(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        height: i32,
        origin: BlockPos,
        foliage_coords: &[FancyFoliageCoords],
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) {
        for end_coord in foliage_coords {
            let base_coord = BlockPos::new(origin.x(), end_coord.branch_base, origin.z());
            if base_coord != end_coord.attachment.pos
                && Self::trim_fancy_tree_branch(height, end_coord.branch_base - origin.y())
            {
                Self::make_fancy_tree_limb(
                    region,
                    registry,
                    random,
                    base_coord,
                    end_coord.attachment.pos,
                    true,
                    config,
                    placement,
                );
            }
        }
    }

    fn place_tree_log_with_axis(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        axis: Axis,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        if !Self::tree_valid_pos(region, pos) {
            return false;
        }

        let state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.trunk_provider,
            pos,
        );
        let state = Self::with_axis_if_present(state, axis);
        placement.set_trunk(region, pos, state);
        true
    }

    fn place_fancy_tree_log(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        axis: Axis,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        Self::place_tree_log_with_axis(region, registry, random, pos, axis, config, placement)
    }

    fn with_axis_if_present(state: BlockStateId, axis: Axis) -> BlockStateId {
        if state.try_get_value(&BlockStateProperties::AXIS).is_some() {
            state.set_value(&BlockStateProperties::AXIS, axis)
        } else {
            state
        }
    }

    fn fancy_tree_shape(height: i32, y: i32) -> f32 {
        if (y as f32) < height as f32 * 0.3 {
            return -1.0;
        }

        let radius = height as f32 / 2.0;
        let adjacent = radius - y as f32;
        let mut distance = (radius * radius - adjacent * adjacent).sqrt();
        if adjacent == 0.0 {
            distance = radius;
        } else if adjacent.abs() >= radius {
            return 0.0;
        }

        distance * 0.5
    }

    fn place_below_trunk_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) {
        let Some(state) = Self::sample_block_state_provider_optional(
            region,
            registry,
            random,
            &config.below_trunk_provider,
            pos,
        ) else {
            return;
        };
        placement.set_trunk(region, pos, state);
    }

    fn place_tree_log(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        if !Self::tree_valid_pos(region, pos) {
            return false;
        }

        let state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.trunk_provider,
            pos,
        );
        placement.set_trunk(region, pos, state);
        true
    }

    fn place_tree_log_if_free(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        if !Self::tree_trunk_placer_is_free(region, pos, &config.trunk_placer) {
            return false;
        }

        Self::place_tree_log(region, registry, random, pos, config, placement)
    }

    fn place_tree_log_growing_through(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        pos: BlockPos,
        can_grow_through: &Identifier,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        if !Self::tree_valid_pos_or_tag(region, pos, can_grow_through) {
            return false;
        }

        let state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.trunk_provider,
            pos,
        );
        placement.set_trunk(region, pos, state);
        true
    }
}

#[derive(Clone, Copy)]
struct FancyFoliageCoords {
    attachment: FoliageAttachment,
    branch_base: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "fancy tree shape tests assert exact vanilla sentinel values"
    )]
    fn fancy_tree_shape_rejects_lower_third() {
        assert_eq!(FeatureDecorationRunner::fancy_tree_shape(12, 3), -1.0);
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "fancy tree shape tests assert exact vanilla crown values"
    )]
    fn fancy_tree_shape_uses_half_circle_crown() {
        assert_eq!(FeatureDecorationRunner::fancy_tree_shape(12, 6), 3.0);
        assert_eq!(FeatureDecorationRunner::fancy_tree_shape(12, 12), 0.0);
    }

    #[test]
    fn fancy_log_axis_prefers_x_on_horizontal_tie() {
        let start = BlockPos::new(0, 0, 0);
        assert_eq!(
            FeatureDecorationRunner::fancy_tree_log_axis(start, BlockPos::new(0, 3, 0)),
            Axis::Y
        );
        assert_eq!(
            FeatureDecorationRunner::fancy_tree_log_axis(start, BlockPos::new(2, 3, 1)),
            Axis::X
        );
        assert_eq!(
            FeatureDecorationRunner::fancy_tree_log_axis(start, BlockPos::new(1, 3, 2)),
            Axis::Z
        );
        assert_eq!(
            FeatureDecorationRunner::fancy_tree_log_axis(start, BlockPos::new(2, 3, 2)),
            Axis::X
        );
    }
}
