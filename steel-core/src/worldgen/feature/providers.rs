use super::prelude::*;
use super::runner::FeatureDecorationRunner;
use smallvec::SmallVec;

impl FeatureDecorationRunner {
    pub(super) fn sample_block_state_provider_optional(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        provider: &BlockStateProvider,
        pos: BlockPos,
    ) -> Option<BlockStateId> {
        match provider {
            BlockStateProvider::RuleBased { fallback, rules } => {
                for rule in rules {
                    if Self::test_block_predicate(region, registry, &rule.if_true, pos) {
                        return Some(Self::sample_block_state_provider(
                            region, registry, random, &rule.then, pos,
                        ));
                    }
                }

                fallback.as_ref().map(|fallback| {
                    Self::sample_block_state_provider(region, registry, random, fallback, pos)
                })
            }
            _ => Some(Self::sample_block_state_provider(
                region, registry, random, provider, pos,
            )),
        }
    }

    pub(super) fn sample_block_state_provider(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        provider: &BlockStateProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        match provider {
            BlockStateProvider::Simple { state } => Self::block_state_from_data(registry, state),
            BlockStateProvider::Weighted { entries } => {
                assert!(
                    !entries.is_empty(),
                    "weighted block-state provider must not be empty"
                );
                let total_weight = entries.iter().fold(0, |total, entry| {
                    assert!(
                        entry.weight > 0,
                        "weighted block-state provider entry weight must be positive, got {}",
                        entry.weight
                    );
                    total + entry.weight
                });
                let mut target = random.next_i32_bounded(total_weight);
                for entry in entries {
                    if target < entry.weight {
                        return Self::block_state_from_data(registry, &entry.data);
                    }
                    target -= entry.weight;
                }
                panic!("weighted block-state provider failed to select an entry");
            }
            BlockStateProvider::RotatedBlock { state } => {
                let state = Self::block_state_from_data(registry, state);
                state.set_value(&BlockStateProperties::AXIS, Self::random_axis(random))
            }
            BlockStateProvider::RandomizedInt {
                property,
                source,
                values,
            } => {
                let state =
                    Self::sample_block_state_provider(region, registry, random, source, pos);
                let value = values.sample(random);
                Self::set_int_property_by_name(registry, state, property, value)
            }
            BlockStateProvider::RuleBased { .. } => {
                if let Some(state) = Self::sample_block_state_provider_optional(
                    region, registry, random, provider, pos,
                ) {
                    state
                } else {
                    region.block_state(pos)
                }
            }
            BlockStateProvider::Noise(provider) => {
                Self::sample_noise_provider(registry, provider, pos)
            }
            BlockStateProvider::NoiseThreshold(provider) => {
                Self::sample_noise_threshold_provider(registry, random, provider, pos)
            }
            BlockStateProvider::DualNoise(provider) => {
                Self::sample_dual_noise_provider(registry, provider, pos)
            }
        }
    }

    pub(super) fn random_axis(random: &mut WorldgenRandom) -> Axis {
        match random.next_i32_bounded(3) {
            0 => Axis::X,
            1 => Axis::Y,
            _ => Axis::Z,
        }
    }

    pub(super) fn set_int_property_by_name(
        registry: &Registry,
        state: BlockStateId,
        property: &str,
        value: i32,
    ) -> BlockStateId {
        let Some(block) = registry.blocks.by_state_id(state) else {
            panic!("block-state provider received invalid block state id {state:?}");
        };
        let value_string = value.to_string();
        let current_properties = registry.blocks.get_properties(state);
        let mut found = false;
        let properties = current_properties
            .iter()
            .map(|(name, existing)| {
                if *name == property {
                    found = true;
                    (*name, value_string.as_str())
                } else {
                    (*name, *existing)
                }
            })
            .collect::<Vec<_>>();

        if !found {
            return state;
        }

        let Some(new_state) = registry
            .blocks
            .state_id_from_block_properties(block, &properties)
        else {
            panic!(
                "randomized int provider produced invalid value {value} for property {property} on {}",
                block.key
            );
        };
        new_state
    }

    pub(super) fn sample_noise_provider(
        registry: &Registry,
        provider: &NoiseProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let noise = Self::normal_noise(&provider.noise, provider.seed);
        let noise_value = Self::noise_value(&noise, pos, provider.scale);
        Self::noise_state_by_value(registry, &provider.states, noise_value)
    }

    pub(super) fn sample_noise_threshold_provider(
        registry: &Registry,
        random: &mut WorldgenRandom,
        provider: &NoiseThresholdProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let noise = Self::normal_noise(&provider.noise, provider.seed);
        let noise_value = Self::noise_value(&noise, pos, provider.scale);
        if noise_value < f64::from(provider.threshold) {
            Self::random_block_state_from_data_list(registry, random, &provider.low_states)
        } else if random.next_f32() < provider.high_chance {
            Self::random_block_state_from_data_list(registry, random, &provider.high_states)
        } else {
            Self::block_state_from_data(registry, &provider.default_state)
        }
    }

    pub(super) fn sample_dual_noise_provider(
        registry: &Registry,
        provider: &DualNoiseProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let slow_noise = Self::normal_noise(&provider.slow_noise, provider.seed);
        let variety_noise = Self::noise_value(&slow_noise, pos, provider.slow_scale);
        let local_variety = Self::clamped_map(
            variety_noise,
            -1.0,
            1.0,
            f64::from(provider.variety[0]),
            f64::from(provider.variety[1] + 1),
        ) as i32;
        assert!(
            local_variety > 0,
            "dual-noise provider local variety must be positive, got {local_variety}"
        );

        let Ok(capacity) = usize::try_from(local_variety) else {
            panic!("dual-noise provider local variety {local_variety} exceeds usize range");
        };
        let mut possible_states = SmallVec::<[BlockStateId; 8]>::with_capacity(capacity);
        for i in 0..local_variety {
            let offset_pos = pos.offset(i * 54_545, 0, i * 34_234);
            let slow_value = Self::noise_value(&slow_noise, offset_pos, provider.slow_scale);
            possible_states.push(Self::noise_state_by_value(
                registry,
                &provider.states,
                slow_value,
            ));
        }

        let noise = Self::normal_noise(&provider.noise, provider.seed);
        let noise_value = Self::noise_value(&noise, pos, provider.scale);
        Self::noise_state_by_resolved_value(&possible_states, noise_value)
    }

    pub(super) fn normal_noise(parameters: &FeatureNoiseParameters, seed: i64) -> NormalNoise {
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(seed as u64));
        NormalNoise::create_from_random(
            &mut random,
            parameters.first_octave,
            &parameters.amplitudes,
        )
    }

    pub(super) fn noise_value(noise: &NormalNoise, pos: BlockPos, scale: f32) -> f64 {
        let scale = f64::from(scale);
        noise.get_value(
            f64::from(pos.x()) * scale,
            f64::from(pos.y()) * scale,
            f64::from(pos.z()) * scale,
        )
    }

    pub(super) fn noise_state_by_value(
        registry: &Registry,
        states: &[BlockStateData],
        noise_value: f64,
    ) -> BlockStateId {
        assert!(
            !states.is_empty(),
            "noise provider state list must not be empty"
        );
        let index = Self::noise_state_index(states.len(), noise_value);
        Self::block_state_from_data(registry, &states[index])
    }

    pub(super) fn noise_state_by_resolved_value(
        states: &[BlockStateId],
        noise_value: f64,
    ) -> BlockStateId {
        assert!(
            !states.is_empty(),
            "noise provider state list must not be empty"
        );
        states[Self::noise_state_index(states.len(), noise_value)]
    }

    pub(super) fn noise_state_index(state_count: usize, noise_value: f64) -> usize {
        let placement_value = f64::midpoint(1.0, noise_value).clamp(0.0, 0.9999);
        (placement_value * state_count as f64) as usize
    }

    pub(super) fn random_block_state_from_data_list(
        registry: &Registry,
        random: &mut WorldgenRandom,
        states: &[BlockStateData],
    ) -> BlockStateId {
        assert!(
            !states.is_empty(),
            "random block-state provider state list must not be empty"
        );
        let Ok(state_count) = i32::try_from(states.len()) else {
            panic!(
                "random block-state provider state count {} exceeds i32 range",
                states.len()
            );
        };
        let index = random.next_i32_bounded(state_count) as usize;
        Self::block_state_from_data(registry, &states[index])
    }

    pub(super) fn clamped_map(
        value: f64,
        from_low: f64,
        from_high: f64,
        to_low: f64,
        to_high: f64,
    ) -> f64 {
        let inverse_lerp = ((value - from_low) / (from_high - from_low)).clamp(0.0, 1.0);
        to_low + inverse_lerp * (to_high - to_low)
    }
}

#[cfg(test)]
mod tests {
    use super::FeatureDecorationRunner;

    #[test]
    fn noise_state_index_uses_vanilla_placement_value_formula() {
        for (state_count, noise_value) in
            [(2, -1.5), (4, -0.5), (8, 0.0), (16, 0.75), (32, 1.5)] as [(usize, f64); 5]
        {
            let placement_value = f64::midpoint(1.0, noise_value).clamp(0.0, 0.9999);
            let expected = (placement_value * state_count as f64) as usize;

            assert_eq!(
                FeatureDecorationRunner::noise_state_index(state_count, noise_value),
                expected
            );
        }
    }
}
