use super::prelude::*;
use super::runner::FeatureDecorationRunner;

struct ConfiguredFeaturePlaceContext<'a, 'region> {
    region: &'a mut WorldGenRegion<'region>,
    registry: &'a Registry,
    random: &'a mut Xoroshiro,
    origin: BlockPos,
    biome_zoom_seed: i64,
}

type ConfiguredFeaturePlacer = for<'a, 'region> fn(
    &mut ConfiguredFeaturePlaceContext<'a, 'region>,
    &ConfiguredFeatureKind,
) -> bool;

struct ConfiguredFeatureRuntimeRegistry {
    placers: FxHashMap<Identifier, ConfiguredFeaturePlacer>,
    /// Placer foundations that compile but are intentionally inactive until a lower-level
    /// worldgen contract is ready.
    pending_placers: FxHashMap<Identifier, ConfiguredFeaturePlacer>,
}

impl ConfiguredFeatureRuntimeRegistry {
    fn new_vanilla() -> Self {
        let mut placers = FxHashMap::default();
        let pending_placers = FxHashMap::default();
        register(
            &mut placers,
            "random_boolean_selector",
            place_random_boolean_selector,
        );
        register(&mut placers, "random_selector", place_random_selector);
        register(
            &mut placers,
            "simple_random_selector",
            place_simple_random_selector,
        );
        register(&mut placers, "bamboo", place_bamboo);
        register(&mut placers, "simple_block", place_simple_block);
        register(&mut placers, "block_blob", place_block_blob);
        register(&mut placers, "block_column", place_block_column);
        register(&mut placers, "block_pile", place_block_pile);
        register(&mut placers, "disk", place_disk);
        register(&mut placers, "basalt_columns", place_basalt_columns);
        register(&mut placers, "basalt_pillar", place_basalt_pillar);
        register(&mut placers, "blue_ice", place_blue_ice);
        register(&mut placers, "bonus_chest", place_bonus_chest);
        register(&mut placers, "chorus_plant", place_chorus_plant);
        register(&mut placers, "coral_claw", place_coral_claw);
        register(&mut placers, "coral_mushroom", place_coral_mushroom);
        register(&mut placers, "coral_tree", place_coral_tree);
        register(&mut placers, "delta_feature", place_delta_feature);
        register(&mut placers, "desert_well", place_desert_well);
        register(&mut placers, "end_gateway", place_end_gateway);
        register(&mut placers, "end_island", place_end_island);
        register(&mut placers, "end_platform", place_end_platform);
        register(&mut placers, "geode", place_geode);
        register(&mut placers, "glowstone_blob", place_glowstone_blob);
        register(
            &mut placers,
            "huge_brown_mushroom",
            place_huge_brown_mushroom,
        );
        register(&mut placers, "huge_red_mushroom", place_huge_red_mushroom);
        register(&mut placers, "huge_fungus", place_huge_fungus);
        register(&mut placers, "iceberg", place_iceberg);
        register(
            &mut placers,
            "nether_forest_vegetation",
            place_nether_forest_vegetation,
        );
        register(
            &mut placers,
            "netherrack_replace_blobs",
            place_netherrack_replace_blobs,
        );
        register(&mut placers, "twisting_vines", place_twisting_vines);
        register(&mut placers, "vines", place_vines);
        register(
            &mut placers,
            "void_start_platform",
            place_void_start_platform,
        );
        register(&mut placers, "weeping_vines", place_weeping_vines);
        register(&mut placers, "spring_feature", place_spring_feature);
        register(&mut placers, "kelp", place_kelp);
        register(&mut placers, "lake", place_lake);
        register(&mut placers, "monster_room", place_monster_room);
        register(&mut placers, "multiface_growth", place_multiface_growth);
        register(&mut placers, "sea_pickle", place_sea_pickle);
        register(&mut placers, "seagrass", place_seagrass);
        register(&mut placers, "underwater_magma", place_underwater_magma);
        register(&mut placers, "pointed_dripstone", place_pointed_dripstone);
        register(&mut placers, "dripstone_cluster", place_dripstone_cluster);
        register(&mut placers, "large_dripstone", place_large_dripstone);
        register(&mut placers, "spike", place_spike);
        register(&mut placers, "ore", place_ore);
        register(&mut placers, "scattered_ore", place_scattered_ore);
        register(&mut placers, "sculk_patch", place_sculk_patch);
        register(&mut placers, "tree", place_tree);
        register(&mut placers, "vegetation_patch", place_vegetation_patch);
        register(
            &mut placers,
            "waterlogged_vegetation_patch",
            place_waterlogged_vegetation_patch,
        );
        register(&mut placers, "fallen_tree", place_fallen_tree);
        register(&mut placers, "fossil", place_fossil);
        register(&mut placers, "freeze_top_layer", place_freeze_top_layer);
        register(&mut placers, "root_system", place_root_system);
        Self {
            placers,
            pending_placers,
        }
    }

    fn placer(&self, feature_type: &Identifier) -> Option<ConfiguredFeaturePlacer> {
        self.placers.get(feature_type).copied()
    }

    fn pending_placer(&self, feature_type: &Identifier) -> Option<ConfiguredFeaturePlacer> {
        self.pending_placers.get(feature_type).copied()
    }
}

static CONFIGURED_FEATURES: LazyLock<ConfiguredFeatureRuntimeRegistry> =
    LazyLock::new(ConfiguredFeatureRuntimeRegistry::new_vanilla);

fn register(
    placers: &mut FxHashMap<Identifier, ConfiguredFeaturePlacer>,
    path: &'static str,
    placer: ConfiguredFeaturePlacer,
) {
    placers.insert(Identifier::new_static("minecraft", path), placer);
}

impl FeatureDecorationRunner {
    pub(super) fn place_configured_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        feature: &ConfiguredFeatureRef,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let kind = Self::configured_feature_kind(registry, feature);
        Self::place_configured_feature_kind(region, registry, random, kind, origin, biome_zoom_seed)
    }

    pub(super) fn place_configured_feature_kind(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        kind: &ConfiguredFeatureKind,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let feature_type = Self::configured_feature_type_id(kind);
        let Some(placer) = CONFIGURED_FEATURES.placer(&feature_type) else {
            if CONFIGURED_FEATURES.pending_placer(&feature_type).is_some() {
                return false;
            }
            // TODO: Register concrete block-mutating feature implementations as they are added.
            return false;
        };
        let mut context = ConfiguredFeaturePlaceContext {
            region,
            registry,
            random,
            origin,
            biome_zoom_seed,
        };
        placer(&mut context, kind)
    }

    pub(super) fn configured_feature_kind<'a>(
        registry: &'a Registry,
        feature: &'a ConfiguredFeatureRef,
    ) -> &'a ConfiguredFeatureKind {
        match feature {
            ConfiguredFeatureRef::Reference(key) => {
                let Some(configured_feature) = registry.configured_features.by_key(key) else {
                    panic!("placed feature references unknown configured feature {key}");
                };
                &configured_feature.kind
            }
            ConfiguredFeatureRef::Inline(configured_feature) => configured_feature,
        }
    }

    fn configured_feature_type_id(kind: &ConfiguredFeatureKind) -> Identifier {
        match kind {
            ConfiguredFeatureKind::Bamboo(_) => Identifier::new_static("minecraft", "bamboo"),
            ConfiguredFeatureKind::BasaltColumns(_) => {
                Identifier::new_static("minecraft", "basalt_columns")
            }
            ConfiguredFeatureKind::BasaltPillar => {
                Identifier::new_static("minecraft", "basalt_pillar")
            }
            ConfiguredFeatureKind::BlockBlob(_) => {
                Identifier::new_static("minecraft", "block_blob")
            }
            ConfiguredFeatureKind::BlockColumn(_) => {
                Identifier::new_static("minecraft", "block_column")
            }
            ConfiguredFeatureKind::BlockPile(_) => {
                Identifier::new_static("minecraft", "block_pile")
            }
            ConfiguredFeatureKind::BlueIce => Identifier::new_static("minecraft", "blue_ice"),
            ConfiguredFeatureKind::BonusChest => Identifier::new_static("minecraft", "bonus_chest"),
            ConfiguredFeatureKind::ChorusPlant => {
                Identifier::new_static("minecraft", "chorus_plant")
            }
            ConfiguredFeatureKind::CoralClaw => Identifier::new_static("minecraft", "coral_claw"),
            ConfiguredFeatureKind::CoralMushroom => {
                Identifier::new_static("minecraft", "coral_mushroom")
            }
            ConfiguredFeatureKind::CoralTree => Identifier::new_static("minecraft", "coral_tree"),
            ConfiguredFeatureKind::DeltaFeature(_) => {
                Identifier::new_static("minecraft", "delta_feature")
            }
            ConfiguredFeatureKind::DesertWell => Identifier::new_static("minecraft", "desert_well"),
            ConfiguredFeatureKind::Disk(_) => Identifier::new_static("minecraft", "disk"),
            ConfiguredFeatureKind::DripstoneCluster(_) => {
                Identifier::new_static("minecraft", "dripstone_cluster")
            }
            ConfiguredFeatureKind::EndGateway(_) => {
                Identifier::new_static("minecraft", "end_gateway")
            }
            ConfiguredFeatureKind::EndIsland => Identifier::new_static("minecraft", "end_island"),
            ConfiguredFeatureKind::EndPlatform => {
                Identifier::new_static("minecraft", "end_platform")
            }
            ConfiguredFeatureKind::EndSpike(_) => Identifier::new_static("minecraft", "end_spike"),
            ConfiguredFeatureKind::FallenTree(_) => {
                Identifier::new_static("minecraft", "fallen_tree")
            }
            ConfiguredFeatureKind::Fossil(_) => Identifier::new_static("minecraft", "fossil"),
            ConfiguredFeatureKind::FreezeTopLayer => {
                Identifier::new_static("minecraft", "freeze_top_layer")
            }
            ConfiguredFeatureKind::Geode(_) => Identifier::new_static("minecraft", "geode"),
            ConfiguredFeatureKind::GlowstoneBlob => {
                Identifier::new_static("minecraft", "glowstone_blob")
            }
            ConfiguredFeatureKind::HugeBrownMushroom(_) => {
                Identifier::new_static("minecraft", "huge_brown_mushroom")
            }
            ConfiguredFeatureKind::HugeFungus(_) => {
                Identifier::new_static("minecraft", "huge_fungus")
            }
            ConfiguredFeatureKind::HugeRedMushroom(_) => {
                Identifier::new_static("minecraft", "huge_red_mushroom")
            }
            ConfiguredFeatureKind::Iceberg(_) => Identifier::new_static("minecraft", "iceberg"),
            ConfiguredFeatureKind::Kelp => Identifier::new_static("minecraft", "kelp"),
            ConfiguredFeatureKind::Lake(_) => Identifier::new_static("minecraft", "lake"),
            ConfiguredFeatureKind::LargeDripstone(_) => {
                Identifier::new_static("minecraft", "large_dripstone")
            }
            ConfiguredFeatureKind::MonsterRoom => {
                Identifier::new_static("minecraft", "monster_room")
            }
            ConfiguredFeatureKind::MultifaceGrowth(_) => {
                Identifier::new_static("minecraft", "multiface_growth")
            }
            ConfiguredFeatureKind::NetherForestVegetation(_) => {
                Identifier::new_static("minecraft", "nether_forest_vegetation")
            }
            ConfiguredFeatureKind::NetherrackReplaceBlobs(_) => {
                Identifier::new_static("minecraft", "netherrack_replace_blobs")
            }
            ConfiguredFeatureKind::Ore(_) => Identifier::new_static("minecraft", "ore"),
            ConfiguredFeatureKind::PointedDripstone(_) => {
                Identifier::new_static("minecraft", "pointed_dripstone")
            }
            ConfiguredFeatureKind::RandomBooleanSelector(_) => {
                Identifier::new_static("minecraft", "random_boolean_selector")
            }
            ConfiguredFeatureKind::RandomSelector(_) => {
                Identifier::new_static("minecraft", "random_selector")
            }
            ConfiguredFeatureKind::RootSystem(_) => {
                Identifier::new_static("minecraft", "root_system")
            }
            ConfiguredFeatureKind::ScatteredOre(_) => {
                Identifier::new_static("minecraft", "scattered_ore")
            }
            ConfiguredFeatureKind::SculkPatch(_) => {
                Identifier::new_static("minecraft", "sculk_patch")
            }
            ConfiguredFeatureKind::SeaPickle(_) => {
                Identifier::new_static("minecraft", "sea_pickle")
            }
            ConfiguredFeatureKind::Seagrass(_) => Identifier::new_static("minecraft", "seagrass"),
            ConfiguredFeatureKind::SimpleBlock(_) => {
                Identifier::new_static("minecraft", "simple_block")
            }
            ConfiguredFeatureKind::SimpleRandomSelector(_) => {
                Identifier::new_static("minecraft", "simple_random_selector")
            }
            ConfiguredFeatureKind::Spike(_) => Identifier::new_static("minecraft", "spike"),
            ConfiguredFeatureKind::SpringFeature(_) => {
                Identifier::new_static("minecraft", "spring_feature")
            }
            ConfiguredFeatureKind::Tree(_) => Identifier::new_static("minecraft", "tree"),
            ConfiguredFeatureKind::TwistingVines(_) => {
                Identifier::new_static("minecraft", "twisting_vines")
            }
            ConfiguredFeatureKind::UnderwaterMagma(_) => {
                Identifier::new_static("minecraft", "underwater_magma")
            }
            ConfiguredFeatureKind::VegetationPatch(_) => {
                Identifier::new_static("minecraft", "vegetation_patch")
            }
            ConfiguredFeatureKind::Vines => Identifier::new_static("minecraft", "vines"),
            ConfiguredFeatureKind::VoidStartPlatform => {
                Identifier::new_static("minecraft", "void_start_platform")
            }
            ConfiguredFeatureKind::WaterloggedVegetationPatch(_) => {
                Identifier::new_static("minecraft", "waterlogged_vegetation_patch")
            }
            ConfiguredFeatureKind::WeepingVines => {
                Identifier::new_static("minecraft", "weeping_vines")
            }
        }
    }
}

fn place_random_boolean_selector(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::RandomBooleanSelector(config) = kind else {
        panic!("random_boolean_selector placer received wrong configured feature kind");
    };
    let selected_feature = if context.random.next_bool() {
        &config.feature_true
    } else {
        &config.feature_false
    };
    FeatureDecorationRunner::place_placed_feature_ref(
        context.region,
        context.registry,
        context.random,
        context.origin,
        selected_feature,
        context.biome_zoom_seed,
    )
}

fn place_random_selector(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::RandomSelector(config) = kind else {
        panic!("random_selector placer received wrong configured feature kind");
    };
    for weighted_feature in &config.features {
        if context.random.next_f32() < weighted_feature.chance {
            return FeatureDecorationRunner::place_placed_feature_ref(
                context.region,
                context.registry,
                context.random,
                context.origin,
                &weighted_feature.feature,
                context.biome_zoom_seed,
            );
        }
    }

    FeatureDecorationRunner::place_placed_feature_ref(
        context.region,
        context.registry,
        context.random,
        context.origin,
        &config.default,
        context.biome_zoom_seed,
    )
}

fn place_simple_random_selector(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::SimpleRandomSelector(config) = kind else {
        panic!("simple_random_selector placer received wrong configured feature kind");
    };
    assert!(
        !config.features.is_empty(),
        "simple random selector feature list must not be empty"
    );
    let Ok(feature_count) = i32::try_from(config.features.len()) else {
        panic!(
            "simple random selector feature count {} exceeds i32 range",
            config.features.len()
        );
    };
    let feature_index = context.random.next_i32_bounded(feature_count) as usize;
    FeatureDecorationRunner::place_placed_feature_ref(
        context.region,
        context.registry,
        context.random,
        context.origin,
        &config.features[feature_index],
        context.biome_zoom_seed,
    )
}

fn place_bamboo(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Bamboo(config) = kind else {
        panic!("bamboo placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_bamboo_feature(
        context.region,
        context.random,
        config,
        context.origin,
    )
}

fn place_simple_block(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::SimpleBlock(config) = kind else {
        panic!("simple_block placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_simple_block_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_block_blob(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::BlockBlob(config) = kind else {
        panic!("block_blob placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_block_blob_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_vegetation_patch(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::VegetationPatch(config) = kind else {
        panic!("vegetation_patch placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_vegetation_patch_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
        context.biome_zoom_seed,
    )
}

fn place_waterlogged_vegetation_patch(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::WaterloggedVegetationPatch(config) = kind else {
        panic!("waterlogged_vegetation_patch placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_waterlogged_vegetation_patch_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
        context.biome_zoom_seed,
    )
}

fn place_block_column(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::BlockColumn(config) = kind else {
        panic!("block_column placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_block_column_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_block_pile(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::BlockPile(config) = kind else {
        panic!("block_pile placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_block_pile_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_disk(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Disk(config) = kind else {
        panic!("disk placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_disk_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_basalt_pillar(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::BasaltPillar = kind else {
        panic!("basalt_pillar placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_basalt_pillar_feature(
        context.region,
        context.random,
        context.origin,
    )
}

fn place_basalt_columns(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::BasaltColumns(config) = kind else {
        panic!("basalt_columns placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_basalt_columns_feature(
        context.region,
        context.random,
        config,
        context.origin,
    )
}

fn place_blue_ice(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::BlueIce = kind else {
        panic!("blue_ice placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_blue_ice_feature(context.region, context.random, context.origin)
}

fn place_bonus_chest(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::BonusChest = kind else {
        panic!("bonus_chest placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_bonus_chest_feature(
        context.region,
        context.random,
        context.origin,
    )
}

fn place_chorus_plant(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::ChorusPlant = kind else {
        panic!("chorus_plant placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_chorus_plant_feature(
        context.region,
        context.registry,
        context.random,
        context.origin,
    )
}

fn place_coral_claw(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::CoralClaw = kind else {
        panic!("coral_claw placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_coral_claw_feature(
        context.region,
        context.registry,
        context.random,
        context.origin,
    )
}

fn place_coral_mushroom(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::CoralMushroom = kind else {
        panic!("coral_mushroom placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_coral_mushroom_feature(
        context.region,
        context.registry,
        context.random,
        context.origin,
    )
}

fn place_coral_tree(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::CoralTree = kind else {
        panic!("coral_tree placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_coral_tree_feature(
        context.region,
        context.registry,
        context.random,
        context.origin,
    )
}

fn place_delta_feature(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::DeltaFeature(config) = kind else {
        panic!("delta_feature placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_delta_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_desert_well(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::DesertWell = kind else {
        panic!("desert_well placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_desert_well_feature(
        context.region,
        context.random,
        context.origin,
    )
}

fn place_end_gateway(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::EndGateway(config) = kind else {
        panic!("end_gateway placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_end_gateway_feature(context.region, config, context.origin)
}

fn place_end_island(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::EndIsland = kind else {
        panic!("end_island placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_end_island_feature(
        context.region,
        context.random,
        context.origin,
    )
}

fn place_end_platform(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::EndPlatform = kind else {
        panic!("end_platform placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_end_platform_feature(context.region, context.origin)
}

fn place_geode(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Geode(config) = kind else {
        panic!("geode placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_geode_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_glowstone_blob(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::GlowstoneBlob = kind else {
        panic!("glowstone_blob placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_glowstone_blob_feature(
        context.region,
        context.random,
        context.origin,
    )
}

fn place_huge_brown_mushroom(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::HugeBrownMushroom(config) = kind else {
        panic!("huge_brown_mushroom placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_huge_brown_mushroom_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_huge_red_mushroom(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::HugeRedMushroom(config) = kind else {
        panic!("huge_red_mushroom placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_huge_red_mushroom_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_huge_fungus(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::HugeFungus(config) = kind else {
        panic!("huge_fungus placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_huge_fungus_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_iceberg(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Iceberg(config) = kind else {
        panic!("iceberg placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_iceberg_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_netherrack_replace_blobs(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::NetherrackReplaceBlobs(config) = kind else {
        panic!("netherrack_replace_blobs placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_netherrack_replace_blobs_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_nether_forest_vegetation(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::NetherForestVegetation(config) = kind else {
        panic!("nether_forest_vegetation placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_nether_forest_vegetation_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_twisting_vines(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::TwistingVines(config) = kind else {
        panic!("twisting_vines placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_twisting_vines_feature(
        context.region,
        context.random,
        config,
        context.origin,
    )
}

fn place_vines(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Vines = kind else {
        panic!("vines placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_vines_feature(context.region, context.origin)
}

fn place_void_start_platform(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::VoidStartPlatform = kind else {
        panic!("void_start_platform placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_void_start_platform_feature(context.region, context.origin)
}

fn place_weeping_vines(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::WeepingVines = kind else {
        panic!("weeping_vines placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_weeping_vines_feature(
        context.region,
        context.random,
        context.origin,
    )
}

fn place_spring_feature(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::SpringFeature(config) = kind else {
        panic!("spring_feature placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_spring_feature(
        context.region,
        context.registry,
        config,
        context.origin,
    )
}

fn place_kelp(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Kelp = kind else {
        panic!("kelp placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_kelp_feature(context.region, context.random, context.origin)
}

fn place_lake(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Lake(config) = kind else {
        panic!("lake placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_lake_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
        context.biome_zoom_seed,
    )
}

fn place_monster_room(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::MonsterRoom = kind else {
        panic!("monster_room placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_monster_room_feature(
        context.region,
        context.registry,
        context.random,
        context.origin,
    )
}

fn place_freeze_top_layer(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::FreezeTopLayer = kind else {
        panic!("freeze_top_layer placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_freeze_top_layer_feature(
        context.region,
        context.registry,
        context.origin,
        context.biome_zoom_seed,
    )
}

fn place_multiface_growth(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::MultifaceGrowth(config) = kind else {
        panic!("multiface_growth placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_multiface_growth_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_sea_pickle(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::SeaPickle(config) = kind else {
        panic!("sea_pickle placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_sea_pickle_feature(
        context.region,
        context.random,
        config,
        context.origin,
    )
}

fn place_seagrass(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Seagrass(config) = kind else {
        panic!("seagrass placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_seagrass_feature(
        context.region,
        context.random,
        config,
        context.origin,
    )
}

fn place_underwater_magma(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::UnderwaterMagma(config) = kind else {
        panic!("underwater_magma placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_underwater_magma_feature(
        context.region,
        context.random,
        config,
        context.origin,
    )
}

fn place_pointed_dripstone(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::PointedDripstone(config) = kind else {
        panic!("pointed_dripstone placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_pointed_dripstone_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_dripstone_cluster(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::DripstoneCluster(config) = kind else {
        panic!("dripstone_cluster placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_dripstone_cluster_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_large_dripstone(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::LargeDripstone(config) = kind else {
        panic!("large_dripstone placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_large_dripstone_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_spike(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Spike(config) = kind else {
        panic!("spike placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_spike_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_ore(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Ore(config) = kind else {
        panic!("ore placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_ore_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_scattered_ore(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::ScatteredOre(config) = kind else {
        panic!("scattered_ore placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_scattered_ore_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_sculk_patch(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::SculkPatch(config) = kind else {
        panic!("sculk_patch placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_sculk_patch_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_tree(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Tree(config) = kind else {
        panic!("tree placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_tree_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
        context.biome_zoom_seed,
    )
}

fn place_fallen_tree(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::FallenTree(config) = kind else {
        panic!("fallen_tree placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_fallen_tree_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
        context.biome_zoom_seed,
    )
}

fn place_fossil(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::Fossil(config) = kind else {
        panic!("fossil placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_fossil_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
    )
}

fn place_root_system(
    context: &mut ConfiguredFeaturePlaceContext<'_, '_>,
    kind: &ConfiguredFeatureKind,
) -> bool {
    let ConfiguredFeatureKind::RootSystem(config) = kind else {
        panic!("root_system placer received wrong configured feature kind");
    };
    FeatureDecorationRunner::place_root_system_feature(
        context.region,
        context.registry,
        context.random,
        config,
        context.origin,
        context.biome_zoom_seed,
    )
}
