use super::{
    BlockStateProvider, DualNoiseProvider, FeatureNoiseParameters, FloatProvider, HeightProvider,
    IntProvider, NoiseProvider, NoiseThresholdProvider, RuleBasedStateProviderRule, TokenStream,
    UniformIntProvider, WeightedBlockState, WeightedIntProvider, generate_block_predicate,
    generate_block_state_data, generate_box, generate_option, generate_vec,
    generate_vertical_anchor, quote,
};

pub(super) fn generate_height_provider(provider: HeightProvider) -> TokenStream {
    match provider {
        HeightProvider::Constant(anchor) => {
            let anchor = generate_vertical_anchor(anchor);
            quote! { HeightProvider::Constant(#anchor) }
        }
        HeightProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::Uniform {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                }
            }
        }
        HeightProvider::Trapezoid {
            min_inclusive,
            max_inclusive,
            plateau,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::Trapezoid {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                    plateau: #plateau,
                }
            }
        }
        HeightProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::BiasedToBottom {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                    inner: #inner,
                }
            }
        }
        HeightProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::VeryBiasedToBottom {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                    inner: #inner,
                }
            }
        }
    }
}

pub(super) fn generate_uniform_int_provider(provider: UniformIntProvider) -> TokenStream {
    let min_inclusive = provider.min_inclusive;
    let max_inclusive = provider.max_inclusive;
    quote! {
        UniformIntProvider {
            min_inclusive: #min_inclusive,
            max_inclusive: #max_inclusive,
        }
    }
}

pub(super) fn generate_weighted_int_provider(provider: &WeightedIntProvider) -> TokenStream {
    let data = generate_int_provider(&provider.data);
    let weight = provider.weight;
    quote! { WeightedIntProvider { data: #data, weight: #weight } }
}

pub(super) fn generate_int_provider(provider: &IntProvider) -> TokenStream {
    match provider {
        IntProvider::Constant(value) => quote! { IntProvider::Constant(#value) },
        IntProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::Uniform {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::BiasedToBottom {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => quote! {
            IntProvider::VeryBiasedToBottom {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
                inner: #inner,
            }
        },
        IntProvider::Trapezoid { min, max, plateau } => quote! {
            IntProvider::Trapezoid {
                min: #min,
                max: #max,
                plateau: #plateau,
            }
        },
        IntProvider::ClampedNormal {
            mean,
            deviation,
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::ClampedNormal {
                mean: #mean,
                deviation: #deviation,
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::Clamped {
            source,
            min_inclusive,
            max_inclusive,
        } => {
            let source = generate_box(source.as_ref(), generate_int_provider);
            quote! {
                IntProvider::Clamped {
                    source: #source,
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                }
            }
        }
        IntProvider::WeightedList { distribution } => {
            let distribution = generate_vec(distribution, generate_weighted_int_provider);
            quote! { IntProvider::WeightedList { distribution: #distribution } }
        }
    }
}

pub(super) fn generate_float_provider(provider: FloatProvider) -> TokenStream {
    match provider {
        FloatProvider::Constant(value) => quote! { FloatProvider::Constant(#value) },
        FloatProvider::Uniform {
            min_inclusive,
            max_exclusive,
        } => quote! {
            FloatProvider::Uniform {
                min_inclusive: #min_inclusive,
                max_exclusive: #max_exclusive,
            }
        },
        FloatProvider::Trapezoid { min, max, plateau } => quote! {
            FloatProvider::Trapezoid {
                min: #min,
                max: #max,
                plateau: #plateau,
            }
        },
        FloatProvider::ClampedNormal {
            mean,
            deviation,
            min,
            max,
        } => quote! {
            FloatProvider::ClampedNormal {
                mean: #mean,
                deviation: #deviation,
                min: #min,
                max: #max,
            }
        },
    }
}

pub(super) fn generate_feature_noise_parameters(
    parameters: &FeatureNoiseParameters,
) -> TokenStream {
    let first_octave = parameters.first_octave;
    let amplitudes = parameters.amplitudes.iter();
    quote! {
        FeatureNoiseParameters {
            first_octave: #first_octave,
            amplitudes: vec![#(#amplitudes),*],
        }
    }
}

pub(super) fn generate_noise_provider(provider: &NoiseProvider) -> TokenStream {
    let noise = generate_feature_noise_parameters(&provider.noise);
    let scale = provider.scale;
    let seed = provider.seed;
    let states = generate_vec(&provider.states, generate_block_state_data);
    quote! {
        NoiseProvider {
            noise: #noise,
            scale: #scale,
            seed: #seed,
            states: #states,
        }
    }
}

pub(super) fn generate_noise_threshold_provider(provider: &NoiseThresholdProvider) -> TokenStream {
    let noise = generate_feature_noise_parameters(&provider.noise);
    let scale = provider.scale;
    let seed = provider.seed;
    let threshold = provider.threshold;
    let high_chance = provider.high_chance;
    let default_state = generate_block_state_data(&provider.default_state);
    let low_states = generate_vec(&provider.low_states, generate_block_state_data);
    let high_states = generate_vec(&provider.high_states, generate_block_state_data);
    quote! {
        NoiseThresholdProvider {
            noise: #noise,
            scale: #scale,
            seed: #seed,
            threshold: #threshold,
            high_chance: #high_chance,
            default_state: #default_state,
            low_states: #low_states,
            high_states: #high_states,
        }
    }
}

pub(super) fn generate_dual_noise_provider(provider: &DualNoiseProvider) -> TokenStream {
    let noise = generate_feature_noise_parameters(&provider.noise);
    let scale = provider.scale;
    let seed = provider.seed;
    let slow_noise = generate_feature_noise_parameters(&provider.slow_noise);
    let slow_scale = provider.slow_scale;
    let states = generate_vec(&provider.states, generate_block_state_data);
    let [variety_min, variety_max] = provider.variety;
    quote! {
        DualNoiseProvider {
            noise: #noise,
            scale: #scale,
            seed: #seed,
            slow_noise: #slow_noise,
            slow_scale: #slow_scale,
            states: #states,
            variety: [#variety_min, #variety_max],
        }
    }
}

pub(super) fn generate_weighted_block_state(entry: &WeightedBlockState) -> TokenStream {
    let data = generate_block_state_data(&entry.data);
    let weight = entry.weight;
    quote! { WeightedBlockState { data: #data, weight: #weight } }
}

pub(super) fn generate_rule_based_state_provider_rule(
    rule: &RuleBasedStateProviderRule,
) -> TokenStream {
    let if_true = generate_block_predicate(&rule.if_true);
    let then = generate_block_state_provider(&rule.then);
    quote! {
        RuleBasedStateProviderRule {
            if_true: #if_true,
            then: #then,
        }
    }
}

pub(super) fn generate_block_state_provider(provider: &BlockStateProvider) -> TokenStream {
    match provider {
        BlockStateProvider::Simple { state } => {
            let state = generate_block_state_data(state);
            quote! { BlockStateProvider::Simple { state: #state } }
        }
        BlockStateProvider::Weighted { entries } => {
            let entries = generate_vec(entries, generate_weighted_block_state);
            quote! { BlockStateProvider::Weighted { entries: #entries } }
        }
        BlockStateProvider::RotatedBlock { state } => {
            let state = generate_block_state_data(state);
            quote! { BlockStateProvider::RotatedBlock { state: #state } }
        }
        BlockStateProvider::RandomizedInt {
            property,
            source,
            values,
        } => {
            let property = property.as_str();
            let source = generate_box(source.as_ref(), generate_block_state_provider);
            let values = generate_int_provider(values);
            quote! {
                BlockStateProvider::RandomizedInt {
                    property: #property.to_string(),
                    source: #source,
                    values: #values,
                }
            }
        }
        BlockStateProvider::RuleBased { fallback, rules } => {
            let fallback = generate_option(fallback, |fallback| {
                generate_box(fallback.as_ref(), generate_block_state_provider)
            });
            let rules = generate_vec(rules, generate_rule_based_state_provider_rule);
            quote! {
                BlockStateProvider::RuleBased {
                    fallback: #fallback,
                    rules: #rules,
                }
            }
        }
        BlockStateProvider::Noise(provider) => {
            let provider = generate_noise_provider(provider);
            quote! { BlockStateProvider::Noise(#provider) }
        }
        BlockStateProvider::NoiseThreshold(provider) => {
            let provider = generate_noise_threshold_provider(provider);
            quote! { BlockStateProvider::NoiseThreshold(#provider) }
        }
        BlockStateProvider::DualNoise(provider) => {
            let provider = generate_dual_noise_provider(provider);
            quote! { BlockStateProvider::DualNoise(#provider) }
        }
    }
}
