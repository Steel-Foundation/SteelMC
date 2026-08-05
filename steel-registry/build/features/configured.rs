use super::{
    ConfiguredFeatureKind, Ident, Span, TokenStream, VegetationPatchConfiguration,
    generate_block_column_layer, generate_block_holder_set, generate_block_predicate,
    generate_block_ref, generate_block_state_data, generate_block_state_provider,
    generate_direction, generate_end_spike, generate_feature_size, generate_float_provider,
    generate_fluid_state_data, generate_foliage_placer, generate_geode_block_settings,
    generate_geode_crack_settings, generate_geode_layer_settings, generate_huge_mushroom_kind,
    generate_identifier, generate_int_provider, generate_offset, generate_option,
    generate_ore_target, generate_placed_feature_ref, generate_root_placer,
    generate_tree_decorator, generate_trunk_placer, generate_vec, generate_vertical_surface,
    generate_weighted_placed_feature, generate_weighted_random_placed_feature,
    generate_weighted_template_entry, quote,
};

#[expect(
    clippy::too_many_lines,
    reason = "keeps feature registry codegen aligned with the runtime feature enum"
)]
pub(super) fn generate_configured_feature_kind(kind: &ConfiguredFeatureKind) -> TokenStream {
    match kind {
        ConfiguredFeatureKind::Bamboo(config) => {
            let probability = config.probability;
            quote! {
                ConfiguredFeatureKind::Bamboo(BambooConfiguration {
                    probability: #probability,
                })
            }
        }
        ConfiguredFeatureKind::BasaltColumns(config) => {
            let height = generate_int_provider(&config.height);
            let reach = generate_int_provider(&config.reach);
            quote! {
                ConfiguredFeatureKind::BasaltColumns(BasaltColumnsConfiguration {
                    height: #height,
                    reach: #reach,
                })
            }
        }
        ConfiguredFeatureKind::BasaltPillar => quote! { ConfiguredFeatureKind::BasaltPillar },
        ConfiguredFeatureKind::BlockBlob(config) => {
            let state = generate_block_state_data(&config.state);
            let can_place_on = generate_block_predicate(&config.can_place_on);
            quote! {
                ConfiguredFeatureKind::BlockBlob(BlockBlobConfiguration {
                    state: #state,
                    can_place_on: #can_place_on,
                })
            }
        }
        ConfiguredFeatureKind::BlockColumn(config) => {
            let direction = generate_direction(config.direction);
            let allowed_placement = generate_block_predicate(&config.allowed_placement);
            let layers = generate_vec(&config.layers, generate_block_column_layer);
            let prioritize_tip = config.prioritize_tip;
            quote! {
                ConfiguredFeatureKind::BlockColumn(BlockColumnConfiguration {
                    direction: #direction,
                    allowed_placement: #allowed_placement,
                    layers: #layers,
                    prioritize_tip: #prioritize_tip,
                })
            }
        }
        ConfiguredFeatureKind::BlockPile(config) => {
            let state_provider = generate_block_state_provider(&config.state_provider);
            quote! {
                ConfiguredFeatureKind::BlockPile(BlockPileConfiguration {
                    state_provider: #state_provider,
                })
            }
        }
        ConfiguredFeatureKind::BlueIce => quote! { ConfiguredFeatureKind::BlueIce },
        ConfiguredFeatureKind::BonusChest => quote! { ConfiguredFeatureKind::BonusChest },
        ConfiguredFeatureKind::ChorusPlant => quote! { ConfiguredFeatureKind::ChorusPlant },
        ConfiguredFeatureKind::CoralClaw => quote! { ConfiguredFeatureKind::CoralClaw },
        ConfiguredFeatureKind::CoralMushroom => quote! { ConfiguredFeatureKind::CoralMushroom },
        ConfiguredFeatureKind::CoralTree => quote! { ConfiguredFeatureKind::CoralTree },
        ConfiguredFeatureKind::DeltaFeature(config) => {
            let contents = generate_block_state_data(&config.contents);
            let rim = generate_block_state_data(&config.rim);
            let size = generate_int_provider(&config.size);
            let rim_size = generate_int_provider(&config.rim_size);
            quote! {
                ConfiguredFeatureKind::DeltaFeature(DeltaFeatureConfiguration {
                    contents: #contents,
                    rim: #rim,
                    size: #size,
                    rim_size: #rim_size,
                })
            }
        }
        ConfiguredFeatureKind::DesertWell => quote! { ConfiguredFeatureKind::DesertWell },
        ConfiguredFeatureKind::Disk(config) => {
            let state_provider = generate_block_state_provider(&config.state_provider);
            let target = generate_block_predicate(&config.target);
            let radius = generate_int_provider(&config.radius);
            let half_height = config.half_height;
            quote! {
                ConfiguredFeatureKind::Disk(DiskConfiguration {
                    state_provider: #state_provider,
                    target: #target,
                    radius: #radius,
                    half_height: #half_height,
                })
            }
        }
        ConfiguredFeatureKind::DripstoneCluster(config) => {
            let floor_to_ceiling_search_range = config.floor_to_ceiling_search_range;
            let height = generate_int_provider(&config.height);
            let radius = generate_int_provider(&config.radius);
            let max_stalagmite_stalactite_height_diff =
                config.max_stalagmite_stalactite_height_diff;
            let height_deviation = config.height_deviation;
            let dripstone_block_layer_thickness =
                generate_int_provider(&config.dripstone_block_layer_thickness);
            let density = generate_float_provider(config.density);
            let wetness = generate_float_provider(config.wetness);
            let chance_of_dripstone_column_at_max_distance_from_center =
                config.chance_of_dripstone_column_at_max_distance_from_center;
            let max_distance_from_center_affecting_height_bias =
                config.max_distance_from_center_affecting_height_bias;
            let max_distance_from_edge_affecting_chance_of_dripstone_column =
                config.max_distance_from_edge_affecting_chance_of_dripstone_column;
            quote! {
                ConfiguredFeatureKind::DripstoneCluster(DripstoneClusterConfiguration {
                    floor_to_ceiling_search_range: #floor_to_ceiling_search_range,
                    height: #height,
                    radius: #radius,
                    max_stalagmite_stalactite_height_diff: #max_stalagmite_stalactite_height_diff,
                    height_deviation: #height_deviation,
                    dripstone_block_layer_thickness: #dripstone_block_layer_thickness,
                    density: #density,
                    wetness: #wetness,
                    chance_of_dripstone_column_at_max_distance_from_center: #chance_of_dripstone_column_at_max_distance_from_center,
                    max_distance_from_center_affecting_height_bias: #max_distance_from_center_affecting_height_bias,
                    max_distance_from_edge_affecting_chance_of_dripstone_column: #max_distance_from_edge_affecting_chance_of_dripstone_column,
                })
            }
        }
        ConfiguredFeatureKind::SpeleothemCluster(config) => {
            let base_block = generate_block_state_data(&config.base_block);
            let pointed_block = generate_block_state_data(&config.pointed_block);
            let replaceable_blocks = generate_block_holder_set(&config.replaceable_blocks);
            let floor_to_ceiling_search_range = config.floor_to_ceiling_search_range;
            let height = generate_int_provider(&config.height);
            let radius = generate_int_provider(&config.radius);
            let max_stalagmite_stalactite_height_diff =
                config.max_stalagmite_stalactite_height_diff;
            let height_deviation = config.height_deviation;
            let speleothem_block_layer_thickness =
                generate_int_provider(&config.speleothem_block_layer_thickness);
            let density = generate_float_provider(config.density);
            let wetness = generate_float_provider(config.wetness);
            let chance_of_speleothem_at_max_distance_from_center =
                config.chance_of_speleothem_at_max_distance_from_center;
            let max_distance_from_edge_affecting_chance_of_speleothem =
                config.max_distance_from_edge_affecting_chance_of_speleothem;
            let max_distance_from_center_affecting_height_bias =
                config.max_distance_from_center_affecting_height_bias;
            quote! {
                ConfiguredFeatureKind::SpeleothemCluster(SpeleothemClusterConfiguration {
                    base_block: #base_block,
                    pointed_block: #pointed_block,
                    replaceable_blocks: #replaceable_blocks,
                    floor_to_ceiling_search_range: #floor_to_ceiling_search_range,
                    height: #height,
                    radius: #radius,
                    max_stalagmite_stalactite_height_diff: #max_stalagmite_stalactite_height_diff,
                    height_deviation: #height_deviation,
                    speleothem_block_layer_thickness: #speleothem_block_layer_thickness,
                    density: #density,
                    wetness: #wetness,
                    chance_of_speleothem_at_max_distance_from_center: #chance_of_speleothem_at_max_distance_from_center,
                    max_distance_from_edge_affecting_chance_of_speleothem: #max_distance_from_edge_affecting_chance_of_speleothem,
                    max_distance_from_center_affecting_height_bias: #max_distance_from_center_affecting_height_bias,
                })
            }
        }
        ConfiguredFeatureKind::EndGateway(config) => {
            let exit = generate_option(&config.exit, generate_offset);
            let exact = config.exact;
            quote! {
                ConfiguredFeatureKind::EndGateway(EndGatewayConfiguration {
                    exit: #exit,
                    exact: #exact,
                })
            }
        }
        ConfiguredFeatureKind::EndIsland => quote! { ConfiguredFeatureKind::EndIsland },
        ConfiguredFeatureKind::EndPlatform => quote! { ConfiguredFeatureKind::EndPlatform },
        ConfiguredFeatureKind::EndSpike(config) => {
            let spikes = generate_vec(&config.spikes, generate_end_spike);
            let crystal_invulnerable = config.crystal_invulnerable;
            let crystal_beam_target = generate_option(&config.crystal_beam_target, generate_offset);
            quote! {
                ConfiguredFeatureKind::EndSpike(EndSpikeConfiguration {
                    spikes: #spikes,
                    crystal_invulnerable: #crystal_invulnerable,
                    crystal_beam_target: #crystal_beam_target,
                })
            }
        }
        ConfiguredFeatureKind::FallenTree(config) => {
            let trunk_provider = generate_block_state_provider(&config.trunk_provider);
            let log_length = generate_int_provider(&config.log_length);
            let stump_decorators = generate_vec(&config.stump_decorators, generate_tree_decorator);
            let log_decorators = generate_vec(&config.log_decorators, generate_tree_decorator);
            quote! {
                ConfiguredFeatureKind::FallenTree(FallenTreeConfiguration {
                    trunk_provider: #trunk_provider,
                    log_length: #log_length,
                    stump_decorators: #stump_decorators,
                    log_decorators: #log_decorators,
                })
            }
        }
        ConfiguredFeatureKind::Fossil(config) => {
            let fossil_structures = generate_vec(&config.fossil_structures, generate_identifier);
            let overlay_structures = generate_vec(&config.overlay_structures, generate_identifier);
            let fossil_processors = generate_identifier(&config.fossil_processors);
            let overlay_processors = generate_identifier(&config.overlay_processors);
            let max_empty_corners_allowed = config.max_empty_corners_allowed;
            quote! {
                ConfiguredFeatureKind::Fossil(FossilConfiguration {
                    fossil_structures: #fossil_structures,
                    overlay_structures: #overlay_structures,
                    fossil_processors: #fossil_processors,
                    overlay_processors: #overlay_processors,
                    max_empty_corners_allowed: #max_empty_corners_allowed,
                })
            }
        }
        ConfiguredFeatureKind::FreezeTopLayer => quote! { ConfiguredFeatureKind::FreezeTopLayer },
        ConfiguredFeatureKind::Geode(config) => {
            let blocks = generate_geode_block_settings(&config.blocks);
            let layers = generate_geode_layer_settings(&config.layers);
            let crack = generate_geode_crack_settings(&config.crack);
            let use_potential_placements_chance = config.use_potential_placements_chance;
            let use_alternate_layer0_chance = config.use_alternate_layer0_chance;
            let placements_require_layer0_alternate = config.placements_require_layer0_alternate;
            let outer_wall_distance = generate_int_provider(&config.outer_wall_distance);
            let distribution_points = generate_int_provider(&config.distribution_points);
            let point_offset = generate_int_provider(&config.point_offset);
            let min_gen_offset = config.min_gen_offset;
            let max_gen_offset = config.max_gen_offset;
            let invalid_blocks_threshold = config.invalid_blocks_threshold;
            let noise_multiplier = config.noise_multiplier;
            quote! {
                ConfiguredFeatureKind::Geode(GeodeConfiguration {
                    blocks: #blocks,
                    layers: #layers,
                    crack: #crack,
                    use_potential_placements_chance: #use_potential_placements_chance,
                    use_alternate_layer0_chance: #use_alternate_layer0_chance,
                    placements_require_layer0_alternate: #placements_require_layer0_alternate,
                    outer_wall_distance: #outer_wall_distance,
                    distribution_points: #distribution_points,
                    point_offset: #point_offset,
                    min_gen_offset: #min_gen_offset,
                    max_gen_offset: #max_gen_offset,
                    invalid_blocks_threshold: #invalid_blocks_threshold,
                    noise_multiplier: #noise_multiplier,
                })
            }
        }
        ConfiguredFeatureKind::GlowstoneBlob => quote! { ConfiguredFeatureKind::GlowstoneBlob },
        ConfiguredFeatureKind::HugeBrownMushroom(config) => {
            generate_huge_mushroom_kind("HugeBrownMushroom", config)
        }
        ConfiguredFeatureKind::HugeFungus(config) => {
            let valid_base_block = generate_block_state_data(&config.valid_base_block);
            let stem_state = generate_block_state_data(&config.stem_state);
            let hat_state = generate_block_state_data(&config.hat_state);
            let decor_state = generate_block_state_data(&config.decor_state);
            let replaceable_blocks = generate_block_predicate(&config.replaceable_blocks);
            let planted = config.planted;
            quote! {
                ConfiguredFeatureKind::HugeFungus(HugeFungusConfiguration {
                    valid_base_block: #valid_base_block,
                    stem_state: #stem_state,
                    hat_state: #hat_state,
                    decor_state: #decor_state,
                    replaceable_blocks: #replaceable_blocks,
                    planted: #planted,
                })
            }
        }
        ConfiguredFeatureKind::HugeRedMushroom(config) => {
            generate_huge_mushroom_kind("HugeRedMushroom", config)
        }
        ConfiguredFeatureKind::Iceberg(state) => {
            let state = generate_block_state_data(state);
            quote! { ConfiguredFeatureKind::Iceberg(#state) }
        }
        ConfiguredFeatureKind::Kelp => quote! { ConfiguredFeatureKind::Kelp },
        ConfiguredFeatureKind::Lake(config) => {
            let fluid = generate_block_state_provider(&config.fluid);
            let barrier = generate_block_state_provider(&config.barrier);
            let can_place_feature = generate_block_predicate(&config.can_place_feature);
            let can_replace_with_air_or_fluid =
                generate_block_predicate(&config.can_replace_with_air_or_fluid);
            let can_replace_with_barrier =
                generate_block_predicate(&config.can_replace_with_barrier);
            quote! {
                ConfiguredFeatureKind::Lake(LakeConfiguration {
                    fluid: #fluid,
                    barrier: #barrier,
                    can_place_feature: #can_place_feature,
                    can_replace_with_air_or_fluid: #can_replace_with_air_or_fluid,
                    can_replace_with_barrier: #can_replace_with_barrier,
                })
            }
        }
        ConfiguredFeatureKind::LargeDripstone(config) => {
            let replaceable_blocks = generate_block_holder_set(&config.replaceable_blocks);
            let floor_to_ceiling_search_range = config.floor_to_ceiling_search_range;
            let column_radius = generate_int_provider(&config.column_radius);
            let height_scale = generate_float_provider(config.height_scale);
            let max_column_radius_to_cave_height_ratio =
                config.max_column_radius_to_cave_height_ratio;
            let stalactite_bluntness = generate_float_provider(config.stalactite_bluntness);
            let stalagmite_bluntness = generate_float_provider(config.stalagmite_bluntness);
            let wind_speed = generate_float_provider(config.wind_speed);
            let min_radius_for_wind = config.min_radius_for_wind;
            let min_bluntness_for_wind = config.min_bluntness_for_wind;
            quote! {
                ConfiguredFeatureKind::LargeDripstone(LargeDripstoneConfiguration {
                    replaceable_blocks: #replaceable_blocks,
                    floor_to_ceiling_search_range: #floor_to_ceiling_search_range,
                    column_radius: #column_radius,
                    height_scale: #height_scale,
                    max_column_radius_to_cave_height_ratio: #max_column_radius_to_cave_height_ratio,
                    stalactite_bluntness: #stalactite_bluntness,
                    stalagmite_bluntness: #stalagmite_bluntness,
                    wind_speed: #wind_speed,
                    min_radius_for_wind: #min_radius_for_wind,
                    min_bluntness_for_wind: #min_bluntness_for_wind,
                })
            }
        }
        ConfiguredFeatureKind::MonsterRoom => quote! { ConfiguredFeatureKind::MonsterRoom },
        ConfiguredFeatureKind::MultifaceGrowth(config) => {
            let block = generate_block_ref(&config.block);
            let search_range = config.search_range;
            let can_place_on_floor = config.can_place_on_floor;
            let can_place_on_ceiling = config.can_place_on_ceiling;
            let can_place_on_wall = config.can_place_on_wall;
            let chance_of_spreading = config.chance_of_spreading;
            let can_be_placed_on = generate_vec(&config.can_be_placed_on, generate_block_ref);
            quote! {
                ConfiguredFeatureKind::MultifaceGrowth(MultifaceGrowthConfiguration {
                    block: #block,
                    search_range: #search_range,
                    can_place_on_floor: #can_place_on_floor,
                    can_place_on_ceiling: #can_place_on_ceiling,
                    can_place_on_wall: #can_place_on_wall,
                    chance_of_spreading: #chance_of_spreading,
                    can_be_placed_on: #can_be_placed_on,
                })
            }
        }
        ConfiguredFeatureKind::NetherForestVegetation(config) => {
            let state_provider = generate_block_state_provider(&config.state_provider);
            let spread_width = config.spread_width;
            let spread_height = config.spread_height;
            quote! {
                ConfiguredFeatureKind::NetherForestVegetation(NetherForestVegetationConfiguration {
                    state_provider: #state_provider,
                    spread_width: #spread_width,
                    spread_height: #spread_height,
                })
            }
        }
        ConfiguredFeatureKind::NetherrackReplaceBlobs(config) => {
            let target = generate_block_state_data(&config.target);
            let state = generate_block_state_data(&config.state);
            let radius = generate_int_provider(&config.radius);
            quote! {
                ConfiguredFeatureKind::NetherrackReplaceBlobs(NetherrackReplaceBlobsConfiguration {
                    target: #target,
                    state: #state,
                    radius: #radius,
                })
            }
        }
        ConfiguredFeatureKind::Ore(config) => {
            let targets = generate_vec(&config.targets, generate_ore_target);
            let size = config.size;
            let discard_chance_on_air_exposure = config.discard_chance_on_air_exposure;
            quote! {
                ConfiguredFeatureKind::Ore(OreConfiguration {
                    targets: #targets,
                    size: #size,
                    discard_chance_on_air_exposure: #discard_chance_on_air_exposure,
                })
            }
        }
        ConfiguredFeatureKind::PointedDripstone(config) => {
            let chance_of_taller_dripstone = config.chance_of_taller_dripstone;
            let chance_of_directional_spread = config.chance_of_directional_spread;
            let chance_of_spread_radius2 = config.chance_of_spread_radius2;
            let chance_of_spread_radius3 = config.chance_of_spread_radius3;
            quote! {
                ConfiguredFeatureKind::PointedDripstone(PointedDripstoneConfiguration {
                    chance_of_taller_dripstone: #chance_of_taller_dripstone,
                    chance_of_directional_spread: #chance_of_directional_spread,
                    chance_of_spread_radius2: #chance_of_spread_radius2,
                    chance_of_spread_radius3: #chance_of_spread_radius3,
                })
            }
        }
        ConfiguredFeatureKind::RandomBooleanSelector(config) => {
            let feature_true = generate_placed_feature_ref(&config.feature_true);
            let feature_false = generate_placed_feature_ref(&config.feature_false);
            quote! {
                ConfiguredFeatureKind::RandomBooleanSelector(RandomBooleanSelectorConfiguration {
                    feature_true: #feature_true,
                    feature_false: #feature_false,
                })
            }
        }
        ConfiguredFeatureKind::RandomSelector(config) => {
            let features = generate_vec(&config.features, generate_weighted_placed_feature);
            let default = generate_placed_feature_ref(&config.default);
            quote! {
                ConfiguredFeatureKind::RandomSelector(RandomSelectorConfiguration {
                    features: #features,
                    default: #default,
                })
            }
        }
        ConfiguredFeatureKind::WeightedRandomSelector(config) => {
            let features = generate_vec(&config.features, generate_weighted_random_placed_feature);
            quote! {
                ConfiguredFeatureKind::WeightedRandomSelector(WeightedRandomFeatureConfiguration {
                    features: #features,
                })
            }
        }
        ConfiguredFeatureKind::RootSystem(config) => {
            let feature = generate_placed_feature_ref(&config.feature);
            let required_vertical_space_for_tree = config.required_vertical_space_for_tree;
            let level_test_distance = config.level_test_distance;
            let max_level_deviation = config.max_level_deviation;
            let root_radius = config.root_radius;
            let root_placement_attempts = config.root_placement_attempts;
            let root_column_max_height = config.root_column_max_height;
            let hanging_root_radius = config.hanging_root_radius;
            let hanging_roots_vertical_span = config.hanging_roots_vertical_span;
            let hanging_root_placement_attempts = config.hanging_root_placement_attempts;
            let allowed_vertical_water_for_tree = config.allowed_vertical_water_for_tree;
            let root_state_provider = generate_block_state_provider(&config.root_state_provider);
            let hanging_root_state_provider =
                generate_block_state_provider(&config.hanging_root_state_provider);
            let root_replaceable = generate_block_holder_set(&config.root_replaceable);
            let allowed_tree_position = generate_block_predicate(&config.allowed_tree_position);
            quote! {
                ConfiguredFeatureKind::RootSystem(RootSystemConfiguration {
                    feature: #feature,
                    required_vertical_space_for_tree: #required_vertical_space_for_tree,
                    level_test_distance: #level_test_distance,
                    max_level_deviation: #max_level_deviation,
                    root_radius: #root_radius,
                    root_placement_attempts: #root_placement_attempts,
                    root_column_max_height: #root_column_max_height,
                    hanging_root_radius: #hanging_root_radius,
                    hanging_roots_vertical_span: #hanging_roots_vertical_span,
                    hanging_root_placement_attempts: #hanging_root_placement_attempts,
                    allowed_vertical_water_for_tree: #allowed_vertical_water_for_tree,
                    root_state_provider: #root_state_provider,
                    hanging_root_state_provider: #hanging_root_state_provider,
                    root_replaceable: #root_replaceable,
                    allowed_tree_position: #allowed_tree_position,
                })
            }
        }
        ConfiguredFeatureKind::ScatteredOre(config) => {
            let targets = generate_vec(&config.targets, generate_ore_target);
            let size = config.size;
            let discard_chance_on_air_exposure = config.discard_chance_on_air_exposure;
            quote! {
                ConfiguredFeatureKind::ScatteredOre(OreConfiguration {
                    targets: #targets,
                    size: #size,
                    discard_chance_on_air_exposure: #discard_chance_on_air_exposure,
                })
            }
        }
        ConfiguredFeatureKind::SculkPatch(config) => {
            let charge_count = config.charge_count;
            let amount_per_charge = config.amount_per_charge;
            let spread_attempts = config.spread_attempts;
            let growth_rounds = config.growth_rounds;
            let spread_rounds = config.spread_rounds;
            let extra_rare_growths = generate_int_provider(&config.extra_rare_growths);
            let catalyst_chance = config.catalyst_chance;
            quote! {
                ConfiguredFeatureKind::SculkPatch(SculkPatchConfiguration {
                    charge_count: #charge_count,
                    amount_per_charge: #amount_per_charge,
                    spread_attempts: #spread_attempts,
                    growth_rounds: #growth_rounds,
                    spread_rounds: #spread_rounds,
                    extra_rare_growths: #extra_rare_growths,
                    catalyst_chance: #catalyst_chance,
                })
            }
        }
        ConfiguredFeatureKind::SeaPickle(config) => {
            let count = generate_int_provider(&config.count);
            quote! {
                ConfiguredFeatureKind::SeaPickle(SeaPickleConfiguration {
                    count: #count,
                })
            }
        }
        ConfiguredFeatureKind::Seagrass(config) => {
            let probability = config.probability;
            quote! {
                ConfiguredFeatureKind::Seagrass(SeagrassConfiguration {
                    probability: #probability,
                })
            }
        }
        ConfiguredFeatureKind::Sequence(config) => {
            let features = generate_vec(&config.features, generate_placed_feature_ref);
            quote! {
                ConfiguredFeatureKind::Sequence(CompositeFeatureConfiguration {
                    features: #features,
                })
            }
        }
        ConfiguredFeatureKind::SimpleBlock(config) => {
            let to_place = generate_block_state_provider(&config.to_place);
            let schedule_tick = config.schedule_tick;
            quote! {
                ConfiguredFeatureKind::SimpleBlock(SimpleBlockConfiguration {
                    to_place: #to_place,
                    schedule_tick: #schedule_tick,
                })
            }
        }
        ConfiguredFeatureKind::SimpleRandomSelector(config) => {
            let features = generate_vec(&config.features, generate_placed_feature_ref);
            quote! {
                ConfiguredFeatureKind::SimpleRandomSelector(SimpleRandomSelectorConfiguration {
                    features: #features,
                })
            }
        }
        ConfiguredFeatureKind::Speleothem(config) => {
            let base_block = generate_block_state_data(&config.base_block);
            let pointed_block = generate_block_state_data(&config.pointed_block);
            let replaceable_blocks = generate_block_holder_set(&config.replaceable_blocks);
            let chance_of_taller_generation = config.chance_of_taller_generation;
            let chance_of_directional_spread = config.chance_of_directional_spread;
            let chance_of_spread_radius2 = config.chance_of_spread_radius2;
            let chance_of_spread_radius3 = config.chance_of_spread_radius3;
            quote! {
                ConfiguredFeatureKind::Speleothem(SpeleothemConfiguration {
                    base_block: #base_block,
                    pointed_block: #pointed_block,
                    replaceable_blocks: #replaceable_blocks,
                    chance_of_taller_generation: #chance_of_taller_generation,
                    chance_of_directional_spread: #chance_of_directional_spread,
                    chance_of_spread_radius2: #chance_of_spread_radius2,
                    chance_of_spread_radius3: #chance_of_spread_radius3,
                })
            }
        }
        ConfiguredFeatureKind::Spike(config) => {
            let state = generate_block_state_data(&config.state);
            let can_place_on = generate_block_predicate(&config.can_place_on);
            let can_replace = generate_block_predicate(&config.can_replace);
            quote! {
                ConfiguredFeatureKind::Spike(SpikeConfiguration {
                    state: #state,
                    can_place_on: #can_place_on,
                    can_replace: #can_replace,
                })
            }
        }
        ConfiguredFeatureKind::SpringFeature(config) => {
            let state = generate_fluid_state_data(&config.state);
            let requires_block_below = config.requires_block_below;
            let rock_count = config.rock_count;
            let hole_count = config.hole_count;
            let valid_blocks = generate_block_holder_set(&config.valid_blocks);
            quote! {
                ConfiguredFeatureKind::SpringFeature(SpringConfiguration {
                    state: #state,
                    requires_block_below: #requires_block_below,
                    rock_count: #rock_count,
                    hole_count: #hole_count,
                    valid_blocks: #valid_blocks,
                })
            }
        }
        ConfiguredFeatureKind::Template(config) => {
            let templates = generate_vec(&config.templates, generate_weighted_template_entry);
            quote! {
                ConfiguredFeatureKind::Template(TemplateFeatureConfiguration {
                    templates: #templates,
                })
            }
        }
        ConfiguredFeatureKind::Tree(config) => {
            let trunk_provider = generate_block_state_provider(&config.trunk_provider);
            let below_trunk_provider = generate_block_state_provider(&config.below_trunk_provider);
            let foliage_provider = generate_block_state_provider(&config.foliage_provider);
            let trunk_placer = generate_trunk_placer(&config.trunk_placer);
            let foliage_placer = generate_foliage_placer(&config.foliage_placer);
            let minimum_size = generate_feature_size(&config.minimum_size);
            let decorators = generate_vec(&config.decorators, generate_tree_decorator);
            let root_placer = generate_option(&config.root_placer, generate_root_placer);
            let ignore_vines = config.ignore_vines;
            quote! {
                ConfiguredFeatureKind::Tree(TreeConfiguration {
                    trunk_provider: #trunk_provider,
                    below_trunk_provider: #below_trunk_provider,
                    foliage_provider: #foliage_provider,
                    trunk_placer: #trunk_placer,
                    foliage_placer: #foliage_placer,
                    minimum_size: #minimum_size,
                    decorators: #decorators,
                    root_placer: #root_placer,
                    ignore_vines: #ignore_vines,
                })
            }
        }
        ConfiguredFeatureKind::TwistingVines(config) => {
            let spread_width = config.spread_width;
            let spread_height = config.spread_height;
            let max_height = config.max_height;
            quote! {
                ConfiguredFeatureKind::TwistingVines(TwistingVinesConfiguration {
                    spread_width: #spread_width,
                    spread_height: #spread_height,
                    max_height: #max_height,
                })
            }
        }
        ConfiguredFeatureKind::UnderwaterMagma(config) => {
            let floor_search_range = config.floor_search_range;
            let placement_radius_around_floor = config.placement_radius_around_floor;
            let placement_probability_per_valid_position =
                config.placement_probability_per_valid_position;
            quote! {
                ConfiguredFeatureKind::UnderwaterMagma(UnderwaterMagmaConfiguration {
                    floor_search_range: #floor_search_range,
                    placement_radius_around_floor: #placement_radius_around_floor,
                    placement_probability_per_valid_position: #placement_probability_per_valid_position,
                })
            }
        }
        ConfiguredFeatureKind::VegetationPatch(config) => {
            generate_vegetation_patch_kind("VegetationPatch", config)
        }
        ConfiguredFeatureKind::Vines => quote! { ConfiguredFeatureKind::Vines },
        ConfiguredFeatureKind::VoidStartPlatform => {
            quote! { ConfiguredFeatureKind::VoidStartPlatform }
        }
        ConfiguredFeatureKind::WaterloggedVegetationPatch(config) => {
            generate_vegetation_patch_kind("WaterloggedVegetationPatch", config)
        }
        ConfiguredFeatureKind::WeepingVines => quote! { ConfiguredFeatureKind::WeepingVines },
    }
}

pub(super) fn generate_vegetation_patch_kind(
    variant_name: &str,
    config: &VegetationPatchConfiguration,
) -> TokenStream {
    let variant = Ident::new(variant_name, Span::call_site());
    let replaceable = generate_identifier(&config.replaceable);
    let ground_state = generate_block_state_provider(&config.ground_state);
    let vegetation_feature = generate_placed_feature_ref(&config.vegetation_feature);
    let surface = generate_vertical_surface(config.surface);
    let depth = generate_int_provider(&config.depth);
    let extra_bottom_block_chance = config.extra_bottom_block_chance;
    let vertical_range = config.vertical_range;
    let vegetation_chance = config.vegetation_chance;
    let xz_radius = generate_int_provider(&config.xz_radius);
    let extra_edge_column_chance = config.extra_edge_column_chance;
    quote! {
        ConfiguredFeatureKind::#variant(VegetationPatchConfiguration {
            replaceable: #replaceable,
            ground_state: #ground_state,
            vegetation_feature: #vegetation_feature,
            surface: #surface,
            depth: #depth,
            extra_bottom_block_chance: #extra_bottom_block_chance,
            vertical_range: #vertical_range,
            vegetation_chance: #vegetation_chance,
            xz_radius: #xz_radius,
            extra_edge_column_chance: #extra_edge_column_chance,
        })
    }
}
