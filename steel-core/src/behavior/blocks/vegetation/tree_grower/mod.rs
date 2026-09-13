//! Vanilla `TreeGrower`: picks the configured tree feature a sapling grows into
//! and places it in a live world.

use std::sync::{Arc, LazyLock};

use rand::{Rng, RngExt};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::feature::{ConfiguredFeature, ConfiguredFeatureKind};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{REGISTRY, vanilla_blocks, vanilla_configured_features};
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};
use steel_worldgen::biomes::obfuscate_biome_seed;

use crate::world::{LevelReader, World};
use crate::worldgen::feature::{FeatureDecorationRunner, no_nested_features};

#[cfg(test)]
mod tests;

type TreeFeature = Option<&'static LazyLock<ConfiguredFeature>>;

/// Vanilla `TreeGrower` constructor arguments.
struct TreeGrowerDefinition {
    secondary_chance: f32,
    mega_tree: TreeFeature,
    secondary_mega_tree: TreeFeature,
    tree: TreeFeature,
    secondary_tree: TreeFeature,
    flowers: TreeFeature,
    secondary_flowers: TreeFeature,
}

static OAK: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.1,
    mega_tree: None,
    secondary_mega_tree: None,
    tree: Some(&vanilla_configured_features::OAK),
    secondary_tree: Some(&vanilla_configured_features::FANCY_OAK),
    flowers: Some(&vanilla_configured_features::OAK_BEES_005),
    secondary_flowers: Some(&vanilla_configured_features::FANCY_OAK_BEES_005),
};

static SPRUCE: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.5,
    mega_tree: Some(&vanilla_configured_features::MEGA_SPRUCE),
    secondary_mega_tree: Some(&vanilla_configured_features::MEGA_PINE),
    tree: Some(&vanilla_configured_features::SPRUCE),
    secondary_tree: None,
    flowers: None,
    secondary_flowers: None,
};

static MANGROVE: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.85,
    mega_tree: None,
    secondary_mega_tree: None,
    tree: Some(&vanilla_configured_features::MANGROVE),
    secondary_tree: Some(&vanilla_configured_features::TALL_MANGROVE),
    flowers: None,
    secondary_flowers: None,
};

static AZALEA: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.0,
    mega_tree: None,
    secondary_mega_tree: None,
    tree: Some(&vanilla_configured_features::AZALEA_TREE),
    secondary_tree: None,
    flowers: None,
    secondary_flowers: None,
};

static BIRCH: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.0,
    mega_tree: None,
    secondary_mega_tree: None,
    tree: Some(&vanilla_configured_features::BIRCH),
    secondary_tree: None,
    flowers: Some(&vanilla_configured_features::BIRCH_BEES_005),
    secondary_flowers: None,
};

static JUNGLE: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.0,
    mega_tree: Some(&vanilla_configured_features::MEGA_JUNGLE_TREE),
    secondary_mega_tree: None,
    tree: Some(&vanilla_configured_features::JUNGLE_TREE_NO_VINE),
    secondary_tree: None,
    flowers: None,
    secondary_flowers: None,
};

static ACACIA: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.0,
    mega_tree: None,
    secondary_mega_tree: None,
    tree: Some(&vanilla_configured_features::ACACIA),
    secondary_tree: None,
    flowers: None,
    secondary_flowers: None,
};

static CHERRY: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.0,
    mega_tree: None,
    secondary_mega_tree: None,
    tree: Some(&vanilla_configured_features::CHERRY),
    secondary_tree: None,
    flowers: Some(&vanilla_configured_features::CHERRY_BEES_005),
    secondary_flowers: None,
};

static DARK_OAK: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.0,
    mega_tree: Some(&vanilla_configured_features::DARK_OAK),
    secondary_mega_tree: None,
    tree: None,
    secondary_tree: None,
    flowers: None,
    secondary_flowers: None,
};

static PALE_OAK: TreeGrowerDefinition = TreeGrowerDefinition {
    secondary_chance: 0.0,
    mega_tree: Some(&vanilla_configured_features::PALE_OAK_BONEMEAL),
    secondary_mega_tree: None,
    tree: None,
    secondary_tree: None,
    flowers: None,
    secondary_flowers: None,
};

/// Vanilla `TreeGrower`, resolved from `tree_grower_name` in `classes.json`.
///
/// The secondary chance lives on the definition like vanilla, not on the block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeGrower {
    /// `TreeGrower.OAK`
    Oak,
    /// `TreeGrower.SPRUCE`
    Spruce,
    /// `TreeGrower.MANGROVE`
    Mangrove,
    /// `TreeGrower.AZALEA`
    Azalea,
    /// `TreeGrower.BIRCH`
    Birch,
    /// `TreeGrower.JUNGLE`
    Jungle,
    /// `TreeGrower.ACACIA`
    Acacia,
    /// `TreeGrower.CHERRY`
    Cherry,
    /// `TreeGrower.DARK_OAK`
    DarkOak,
    /// `TreeGrower.PALE_OAK`
    PaleOak,
}

impl TreeGrower {
    fn definition(self) -> &'static TreeGrowerDefinition {
        match self {
            Self::Oak => &OAK,
            Self::Spruce => &SPRUCE,
            Self::Mangrove => &MANGROVE,
            Self::Azalea => &AZALEA,
            Self::Birch => &BIRCH,
            Self::Jungle => &JUNGLE,
            Self::Acacia => &ACACIA,
            Self::Cherry => &CHERRY,
            Self::DarkOak => &DARK_OAK,
            Self::PaleOak => &PALE_OAK,
        }
    }

    /// Vanilla `TreeGrower.getConfiguredFeature`.
    ///
    /// The secondary roll is always consumed, even for growers without a secondary tree.
    fn get_configured_feature(
        self,
        rng: &mut dyn Rng,
        has_flowers: bool,
    ) -> Option<&'static ConfiguredFeature> {
        let definition = self.definition();
        if rng.random::<f32>() < definition.secondary_chance {
            if has_flowers && let Some(feature) = definition.secondary_flowers {
                return Some(feature);
            }
            if let Some(feature) = definition.secondary_tree {
                return Some(feature);
            }
        }
        if has_flowers && let Some(feature) = definition.flowers {
            return Some(feature);
        }
        definition.tree.map(|feature| &**feature)
    }

    /// Vanilla `TreeGrower.getConfiguredMegaFeature`.
    ///
    /// Unlike the single-tree selection, the roll only happens when a secondary mega tree exists.
    fn get_configured_mega_feature(self, rng: &mut dyn Rng) -> Option<&'static ConfiguredFeature> {
        let definition = self.definition();
        if let Some(feature) = definition.secondary_mega_tree
            && rng.random::<f32>() < definition.secondary_chance
        {
            return Some(feature);
        }
        definition.mega_tree.map(|feature| &**feature)
    }

    /// Vanilla `TreeGrower.getMinimumHeight`: the base trunk height of the primary tree.
    pub(crate) fn minimum_height(self) -> Option<i32> {
        let feature = self.definition().tree?;
        let ConfiguredFeatureKind::Tree(config) = &feature.kind else {
            return None;
        };
        Some(config.trunk_placer.base_height())
    }

    /// Vanilla `TreeGrower.growTree`.
    ///
    /// A two-by-two sapling square grows the mega tree, and a failed mega placement restores
    /// the saplings without falling back to a single tree.
    pub(crate) fn grow_tree(
        self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        rng: &mut dyn Rng,
    ) -> bool {
        if let Some(mega_feature) = self.get_configured_mega_feature(rng) {
            for dx in [0, -1] {
                for dz in [0, -1] {
                    if !Self::is_two_by_two_sapling(state, world.as_ref(), pos, dx, dz) {
                        continue;
                    }

                    let origin = pos.offset(dx, 0, dz);
                    let square = [origin, origin.east(), origin.south(), origin.east().south()];
                    for sapling_pos in square {
                        world.set_block(
                            sapling_pos,
                            vanilla_blocks::AIR.default_state(),
                            UpdateFlags::UPDATE_NONE,
                        );
                    }
                    if Self::place(world, mega_feature, origin, rng) {
                        return true;
                    }
                    for sapling_pos in square {
                        world.set_block(sapling_pos, state, UpdateFlags::UPDATE_NONE);
                    }
                    return false;
                }
            }
        }

        let has_flowers = Self::has_flowers(world.as_ref(), pos);
        let Some(feature) = self.get_configured_feature(rng, has_flowers) else {
            return false;
        };

        let fluid = state.get_fluid_state();
        let empty_block = if fluid.is_empty() {
            vanilla_blocks::AIR.default_state()
        } else {
            fluid.create_legacy_block()
        };
        world.set_block(pos, empty_block, UpdateFlags::UPDATE_NONE);
        if Self::place(world, feature, pos, rng) {
            if world.get_block_state(pos) == empty_block {
                world.send_block_updated(pos);
            }
            return true;
        }
        world.set_block(pos, state, UpdateFlags::UPDATE_NONE);
        false
    }

    /// Vanilla `ConfiguredFeature.place` for the tree features a grower references.
    fn place(
        world: &Arc<World>,
        feature: &ConfiguredFeature,
        origin: BlockPos,
        rng: &mut dyn Rng,
    ) -> bool {
        let ConfiguredFeatureKind::Tree(config) = &feature.kind else {
            return false;
        };
        let mut worldgen_random = WorldgenRandom::from_seed(rng.random());
        let mut level = Arc::clone(world);
        FeatureDecorationRunner::place_tree_feature(
            &mut level,
            &REGISTRY,
            &mut worldgen_random,
            config,
            origin,
            obfuscate_biome_seed(world.seed()),
            no_nested_features,
        )
    }

    /// Vanilla `TreeGrower.isTwoByTwoSapling`.
    fn is_two_by_two_sapling(
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        dx: i32,
        dz: i32,
    ) -> bool {
        let block = state.get_block();
        [(dx, dz), (dx + 1, dz), (dx, dz + 1), (dx + 1, dz + 1)]
            .into_iter()
            .all(|(x, z)| world.get_block_state(pos.offset(x, 0, z)).get_block() == block)
    }

    /// Vanilla `TreeGrower.hasFlowers`: any `#minecraft:flowers` block within the 5x3x5 box.
    fn has_flowers(world: &dyn LevelReader, pos: BlockPos) -> bool {
        (-2..=2).any(|x| {
            (-1..=1).any(|y| {
                (-2..=2).any(|z| {
                    world
                        .get_block_state(pos.offset(x, y, z))
                        .get_block()
                        .has_tag(&BlockTag::FLOWERS)
                })
            })
        })
    }
}
