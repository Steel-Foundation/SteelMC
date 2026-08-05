use super::{
    AboveRootPlacement, BlobFoliagePlacer, BlockColumnLayer, EndSpike, FeatureSize, FoliagePlacer,
    FoliagePlacerBase, GeodeBlockSettings, GeodeCrackSettings, GeodeLayerSettings,
    HugeMushroomConfiguration, Ident, MangroveRootPlacement, OreTarget, RootPlacer, RuleTest, Span,
    TemplateEntry, TokenStream, TreeDecorator, TrunkPlacer, TrunkPlacerBase, VerticalSurface,
    WeightedPlacedFeature, WeightedRandomPlacedFeature, WeightedTemplateEntry,
    generate_block_predicate, generate_block_ref, generate_block_state_data,
    generate_block_state_provider, generate_direction, generate_identifier, generate_int_provider,
    generate_option, generate_placed_feature_ref, generate_rotation, generate_uniform_int_provider,
    generate_vec, quote,
};

pub(super) fn generate_block_column_layer(layer: &BlockColumnLayer) -> TokenStream {
    let height = generate_int_provider(&layer.height);
    let provider = generate_block_state_provider(&layer.provider);
    quote! { BlockColumnLayer { height: #height, provider: #provider } }
}

pub(super) fn generate_end_spike(spike: &EndSpike) -> TokenStream {
    let center_x = spike.center_x;
    let center_z = spike.center_z;
    let radius = spike.radius;
    let height = spike.height;
    let guarded = spike.guarded;
    quote! {
        EndSpike {
            center_x: #center_x,
            center_z: #center_z,
            radius: #radius,
            height: #height,
            guarded: #guarded,
        }
    }
}

pub(super) fn generate_geode_block_settings(settings: &GeodeBlockSettings) -> TokenStream {
    let filling_provider = generate_block_state_provider(&settings.filling_provider);
    let inner_layer_provider = generate_block_state_provider(&settings.inner_layer_provider);
    let alternate_inner_layer_provider =
        generate_block_state_provider(&settings.alternate_inner_layer_provider);
    let middle_layer_provider = generate_block_state_provider(&settings.middle_layer_provider);
    let outer_layer_provider = generate_block_state_provider(&settings.outer_layer_provider);
    let inner_placements = generate_vec(&settings.inner_placements, generate_block_state_data);
    let cannot_replace = generate_identifier(&settings.cannot_replace);
    let invalid_blocks = generate_identifier(&settings.invalid_blocks);
    quote! {
        GeodeBlockSettings {
            filling_provider: #filling_provider,
            inner_layer_provider: #inner_layer_provider,
            alternate_inner_layer_provider: #alternate_inner_layer_provider,
            middle_layer_provider: #middle_layer_provider,
            outer_layer_provider: #outer_layer_provider,
            inner_placements: #inner_placements,
            cannot_replace: #cannot_replace,
            invalid_blocks: #invalid_blocks,
        }
    }
}

pub(super) fn generate_geode_layer_settings(settings: &GeodeLayerSettings) -> TokenStream {
    let filling = settings.filling;
    let inner_layer = settings.inner_layer;
    let middle_layer = settings.middle_layer;
    let outer_layer = settings.outer_layer;
    quote! {
        GeodeLayerSettings {
            filling: #filling,
            inner_layer: #inner_layer,
            middle_layer: #middle_layer,
            outer_layer: #outer_layer,
        }
    }
}

pub(super) fn generate_geode_crack_settings(settings: &GeodeCrackSettings) -> TokenStream {
    let generate_crack_chance = settings.generate_crack_chance;
    let base_crack_size = settings.base_crack_size;
    let crack_point_offset = settings.crack_point_offset;
    quote! {
        GeodeCrackSettings {
            generate_crack_chance: #generate_crack_chance,
            base_crack_size: #base_crack_size,
            crack_point_offset: #crack_point_offset,
        }
    }
}

pub(super) fn generate_ore_target(target: &OreTarget) -> TokenStream {
    let target_rule = generate_rule_test(&target.target);
    let state = generate_block_state_data(&target.state);
    quote! { OreTarget { target: #target_rule, state: #state } }
}

pub(super) fn generate_rule_test(rule: &RuleTest) -> TokenStream {
    match rule {
        RuleTest::BlockMatch { block } => {
            let block = generate_block_ref(block);
            quote! { RuleTest::BlockMatch { block: #block } }
        }
        RuleTest::TagMatch { tag } => {
            let tag = generate_identifier(tag);
            quote! { RuleTest::TagMatch { tag: #tag } }
        }
    }
}

pub(super) fn generate_weighted_placed_feature(feature: &WeightedPlacedFeature) -> TokenStream {
    let chance = feature.chance;
    let feature = generate_placed_feature_ref(&feature.feature);
    quote! { WeightedPlacedFeature { chance: #chance, feature: #feature } }
}

pub(super) fn generate_weighted_random_placed_feature(
    feature: &WeightedRandomPlacedFeature,
) -> TokenStream {
    let data = generate_placed_feature_ref(&feature.data);
    let weight = feature.weight;
    quote! { WeightedRandomPlacedFeature { data: #data, weight: #weight } }
}

pub(super) fn generate_template_entry(entry: &TemplateEntry) -> TokenStream {
    let id = generate_identifier(&entry.id);
    let rotations = generate_vec(&entry.rotations, |rotation| generate_rotation(*rotation));
    quote! { TemplateEntry { id: #id, rotations: #rotations } }
}

pub(super) fn generate_weighted_template_entry(entry: &WeightedTemplateEntry) -> TokenStream {
    let data = generate_template_entry(&entry.data);
    let weight = entry.weight;
    quote! { WeightedTemplateEntry { data: #data, weight: #weight } }
}

pub(super) fn generate_trunk_placer_base(base: &TrunkPlacerBase) -> TokenStream {
    let base_height = base.base_height;
    let height_rand_a = base.height_rand_a;
    let height_rand_b = base.height_rand_b;
    quote! {
        TrunkPlacerBase {
            base_height: #base_height,
            height_rand_a: #height_rand_a,
            height_rand_b: #height_rand_b,
        }
    }
}

pub(super) fn generate_trunk_placer(placer: &TrunkPlacer) -> TokenStream {
    match placer {
        TrunkPlacer::Straight(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Straight(#base) }
        }
        TrunkPlacer::Giant(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Giant(#base) }
        }
        TrunkPlacer::Fancy(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Fancy(#base) }
        }
        TrunkPlacer::Forking(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Forking(#base) }
        }
        TrunkPlacer::DarkOak(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::DarkOak(#base) }
        }
        TrunkPlacer::MegaJungle(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::MegaJungle(#base) }
        }
        TrunkPlacer::Bending(placer) => {
            let base_height = placer.base_height;
            let height_rand_a = placer.height_rand_a;
            let height_rand_b = placer.height_rand_b;
            let min_height_for_leaves = placer.min_height_for_leaves;
            let bend_length = generate_int_provider(&placer.bend_length);
            quote! {
                TrunkPlacer::Bending(BendingTrunkPlacer {
                    base_height: #base_height,
                    height_rand_a: #height_rand_a,
                    height_rand_b: #height_rand_b,
                    min_height_for_leaves: #min_height_for_leaves,
                    bend_length: #bend_length,
                })
            }
        }
        TrunkPlacer::UpwardsBranching(placer) => {
            let base_height = placer.base_height;
            let height_rand_a = placer.height_rand_a;
            let height_rand_b = placer.height_rand_b;
            let extra_branch_steps = generate_int_provider(&placer.extra_branch_steps);
            let extra_branch_length = generate_int_provider(&placer.extra_branch_length);
            let place_branch_per_log_probability = placer.place_branch_per_log_probability;
            let can_grow_through = generate_identifier(&placer.can_grow_through);
            quote! {
                TrunkPlacer::UpwardsBranching(UpwardsBranchingTrunkPlacer {
                    base_height: #base_height,
                    height_rand_a: #height_rand_a,
                    height_rand_b: #height_rand_b,
                    extra_branch_steps: #extra_branch_steps,
                    extra_branch_length: #extra_branch_length,
                    place_branch_per_log_probability: #place_branch_per_log_probability,
                    can_grow_through: #can_grow_through,
                })
            }
        }
        TrunkPlacer::Cherry(placer) => {
            let base_height = placer.base_height;
            let height_rand_a = placer.height_rand_a;
            let height_rand_b = placer.height_rand_b;
            let branch_count = generate_int_provider(&placer.branch_count);
            let branch_horizontal_length = generate_int_provider(&placer.branch_horizontal_length);
            let branch_start_offset_from_top =
                generate_uniform_int_provider(placer.branch_start_offset_from_top);
            let branch_end_offset_from_top =
                generate_int_provider(&placer.branch_end_offset_from_top);
            quote! {
                TrunkPlacer::Cherry(CherryTrunkPlacer {
                    base_height: #base_height,
                    height_rand_a: #height_rand_a,
                    height_rand_b: #height_rand_b,
                    branch_count: #branch_count,
                    branch_horizontal_length: #branch_horizontal_length,
                    branch_start_offset_from_top: #branch_start_offset_from_top,
                    branch_end_offset_from_top: #branch_end_offset_from_top,
                })
            }
        }
    }
}

pub(super) fn generate_blob_foliage_placer(placer: &BlobFoliagePlacer) -> TokenStream {
    let radius = generate_int_provider(&placer.radius);
    let offset = generate_int_provider(&placer.offset);
    let height = generate_int_provider(&placer.height);
    quote! { BlobFoliagePlacer { radius: #radius, offset: #offset, height: #height } }
}

pub(super) fn generate_foliage_placer_base(placer: &FoliagePlacerBase) -> TokenStream {
    let radius = generate_int_provider(&placer.radius);
    let offset = generate_int_provider(&placer.offset);
    quote! { FoliagePlacerBase { radius: #radius, offset: #offset } }
}

pub(super) fn generate_foliage_placer(placer: &FoliagePlacer) -> TokenStream {
    match placer {
        FoliagePlacer::Blob(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Blob(#placer) }
        }
        FoliagePlacer::Spruce(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let trunk_height = generate_int_provider(&placer.trunk_height);
            quote! {
                FoliagePlacer::Spruce(SpruceFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    trunk_height: #trunk_height,
                })
            }
        }
        FoliagePlacer::Pine(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let height = generate_int_provider(&placer.height);
            quote! {
                FoliagePlacer::Pine(PineFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    height: #height,
                })
            }
        }
        FoliagePlacer::Acacia(placer) => {
            let placer = generate_foliage_placer_base(placer);
            quote! { FoliagePlacer::Acacia(#placer) }
        }
        FoliagePlacer::Bush(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Bush(#placer) }
        }
        FoliagePlacer::Fancy(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Fancy(#placer) }
        }
        FoliagePlacer::Jungle(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Jungle(#placer) }
        }
        FoliagePlacer::MegaPine(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let crown_height = generate_int_provider(&placer.crown_height);
            quote! {
                FoliagePlacer::MegaPine(MegaPineFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    crown_height: #crown_height,
                })
            }
        }
        FoliagePlacer::DarkOak(placer) => {
            let placer = generate_foliage_placer_base(placer);
            quote! { FoliagePlacer::DarkOak(#placer) }
        }
        FoliagePlacer::RandomSpread(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let foliage_height = placer.foliage_height;
            let leaf_placement_attempts = placer.leaf_placement_attempts;
            quote! {
                FoliagePlacer::RandomSpread(RandomSpreadFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    foliage_height: #foliage_height,
                    leaf_placement_attempts: #leaf_placement_attempts,
                })
            }
        }
        FoliagePlacer::Cherry(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let height = generate_int_provider(&placer.height);
            let wide_bottom_layer_hole_chance = placer.wide_bottom_layer_hole_chance;
            let corner_hole_chance = placer.corner_hole_chance;
            let hanging_leaves_chance = placer.hanging_leaves_chance;
            let hanging_leaves_extension_chance = placer.hanging_leaves_extension_chance;
            quote! {
                FoliagePlacer::Cherry(CherryFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    height: #height,
                    wide_bottom_layer_hole_chance: #wide_bottom_layer_hole_chance,
                    corner_hole_chance: #corner_hole_chance,
                    hanging_leaves_chance: #hanging_leaves_chance,
                    hanging_leaves_extension_chance: #hanging_leaves_extension_chance,
                })
            }
        }
    }
}

pub(super) fn generate_feature_size(size: &FeatureSize) -> TokenStream {
    match size {
        FeatureSize::TwoLayers(size) => {
            let limit = size.limit;
            let lower_size = size.lower_size;
            let upper_size = size.upper_size;
            let min_clipped_height =
                generate_option(&size.min_clipped_height, |value| quote! { #value });
            quote! {
                FeatureSize::TwoLayers(TwoLayersFeatureSize {
                    limit: #limit,
                    lower_size: #lower_size,
                    upper_size: #upper_size,
                    min_clipped_height: #min_clipped_height,
                })
            }
        }
        FeatureSize::ThreeLayers(size) => {
            let limit = size.limit;
            let lower_size = size.lower_size;
            let middle_size = size.middle_size;
            let upper_limit = size.upper_limit;
            let upper_size = size.upper_size;
            let min_clipped_height =
                generate_option(&size.min_clipped_height, |value| quote! { #value });
            quote! {
                FeatureSize::ThreeLayers(ThreeLayersFeatureSize {
                    limit: #limit,
                    lower_size: #lower_size,
                    middle_size: #middle_size,
                    upper_limit: #upper_limit,
                    upper_size: #upper_size,
                    min_clipped_height: #min_clipped_height,
                })
            }
        }
    }
}

pub(super) fn generate_above_root_placement(placement: &AboveRootPlacement) -> TokenStream {
    let above_root_provider = generate_block_state_provider(&placement.above_root_provider);
    let above_root_placement_chance = placement.above_root_placement_chance;
    quote! {
        AboveRootPlacement {
            above_root_provider: #above_root_provider,
            above_root_placement_chance: #above_root_placement_chance,
        }
    }
}

pub(super) fn generate_mangrove_root_placement(placement: &MangroveRootPlacement) -> TokenStream {
    let can_grow_through = generate_identifier(&placement.can_grow_through);
    let muddy_roots_in = generate_vec(&placement.muddy_roots_in, generate_identifier);
    let muddy_roots_provider = generate_block_state_provider(&placement.muddy_roots_provider);
    let max_root_width = placement.max_root_width;
    let max_root_length = placement.max_root_length;
    let random_skew_chance = placement.random_skew_chance;
    quote! {
        MangroveRootPlacement {
            can_grow_through: #can_grow_through,
            muddy_roots_in: #muddy_roots_in,
            muddy_roots_provider: #muddy_roots_provider,
            max_root_width: #max_root_width,
            max_root_length: #max_root_length,
            random_skew_chance: #random_skew_chance,
        }
    }
}

pub(super) fn generate_root_placer(placer: &RootPlacer) -> TokenStream {
    match placer {
        RootPlacer::Mangrove(placer) => {
            let trunk_offset_y = generate_int_provider(&placer.trunk_offset_y);
            let root_provider = generate_block_state_provider(&placer.root_provider);
            let above_root_placement = generate_above_root_placement(&placer.above_root_placement);
            let mangrove_root_placement =
                generate_mangrove_root_placement(&placer.mangrove_root_placement);
            quote! {
                RootPlacer::Mangrove(MangroveRootPlacer {
                    trunk_offset_y: #trunk_offset_y,
                    root_provider: #root_provider,
                    above_root_placement: #above_root_placement,
                    mangrove_root_placement: #mangrove_root_placement,
                })
            }
        }
    }
}

pub(super) fn generate_tree_decorator(decorator: &TreeDecorator) -> TokenStream {
    match decorator {
        TreeDecorator::AlterGround { provider } => {
            let provider = generate_block_state_provider(provider);
            quote! { TreeDecorator::AlterGround { provider: #provider } }
        }
        TreeDecorator::Beehive { probability } => {
            quote! { TreeDecorator::Beehive { probability: #probability } }
        }
        TreeDecorator::Cocoa { probability } => {
            quote! { TreeDecorator::Cocoa { probability: #probability } }
        }
        TreeDecorator::CreakingHeart { probability } => {
            quote! { TreeDecorator::CreakingHeart { probability: #probability } }
        }
        TreeDecorator::LeaveVine { probability } => {
            quote! { TreeDecorator::LeaveVine { probability: #probability } }
        }
        TreeDecorator::TrunkVine => quote! { TreeDecorator::TrunkVine },
        TreeDecorator::AttachedToLeaves(decorator) => {
            let probability = decorator.probability;
            let exclusion_radius_xz = decorator.exclusion_radius_xz;
            let exclusion_radius_y = decorator.exclusion_radius_y;
            let required_empty_blocks = decorator.required_empty_blocks;
            let block_provider = generate_block_state_provider(&decorator.block_provider);
            let directions = generate_vec(&decorator.directions, |direction| {
                generate_direction(*direction)
            });
            quote! {
                TreeDecorator::AttachedToLeaves(AttachedToLeavesDecorator {
                    probability: #probability,
                    exclusion_radius_xz: #exclusion_radius_xz,
                    exclusion_radius_y: #exclusion_radius_y,
                    required_empty_blocks: #required_empty_blocks,
                    block_provider: #block_provider,
                    directions: #directions,
                })
            }
        }
        TreeDecorator::AttachedToLogs(decorator) => {
            let probability = decorator.probability;
            let block_provider = generate_block_state_provider(&decorator.block_provider);
            let directions = generate_vec(&decorator.directions, |direction| {
                generate_direction(*direction)
            });
            quote! {
                TreeDecorator::AttachedToLogs(AttachedToLogsDecorator {
                    probability: #probability,
                    block_provider: #block_provider,
                    directions: #directions,
                })
            }
        }
        TreeDecorator::PlaceOnGround(decorator) => {
            let block_state_provider =
                generate_block_state_provider(&decorator.block_state_provider);
            let tries = decorator.tries;
            let radius = decorator.radius;
            let height = decorator.height;
            quote! {
                TreeDecorator::PlaceOnGround(PlaceOnGroundDecorator {
                    block_state_provider: #block_state_provider,
                    tries: #tries,
                    radius: #radius,
                    height: #height,
                })
            }
        }
        TreeDecorator::PaleMoss {
            leaves_probability,
            trunk_probability,
            ground_probability,
        } => quote! {
            TreeDecorator::PaleMoss {
                leaves_probability: #leaves_probability,
                trunk_probability: #trunk_probability,
                ground_probability: #ground_probability,
            }
        },
    }
}

pub(super) fn generate_vertical_surface(surface: VerticalSurface) -> TokenStream {
    match surface {
        VerticalSurface::Floor => quote! { VerticalSurface::Floor },
        VerticalSurface::Ceiling => quote! { VerticalSurface::Ceiling },
    }
}

pub(super) fn generate_huge_mushroom_kind(
    variant_name: &str,
    config: &HugeMushroomConfiguration,
) -> TokenStream {
    let variant = Ident::new(variant_name, Span::call_site());
    let cap_provider = generate_block_state_provider(&config.cap_provider);
    let stem_provider = generate_block_state_provider(&config.stem_provider);
    let foliage_radius = config.foliage_radius;
    let can_place_on = generate_block_predicate(&config.can_place_on);
    quote! {
        ConfiguredFeatureKind::#variant(HugeMushroomConfiguration {
            cap_provider: #cap_provider,
            stem_provider: #stem_provider,
            foliage_radius: #foliage_radius,
            can_place_on: #can_place_on,
        })
    }
}
