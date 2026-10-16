use super::BlockStateProviderEvaluator;
use crate::world::LevelReader;
use smallvec::SmallVec;
use steel_math::map_clamped;
use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
};
use steel_registry::feature::{
    BlockHolderSet, BlockStateData, BlockStateProviderKind, DualNoiseProvider,
    FeatureNoiseParameters, NoiseProvider, NoiseThresholdProvider,
};
use steel_registry::{Registry, TaggedRegistryExt as _};
use steel_utils::random::{Random, RandomSource, legacy_random::LegacyRandom};
use steel_utils::{BlockPos, BlockStateId, Direction};
use steel_worldgen::noise::NormalNoise;

pub(crate) trait NoiseScale: Copy {
    fn scale_coord(self, coord: i32) -> f64;
}

impl NoiseScale for f32 {
    fn scale_coord(self, coord: i32) -> f64 {
        f64::from(coord as f32 * self)
    }
}

impl NoiseScale for f64 {
    fn scale_coord(self, coord: i32) -> f64 {
        f64::from(coord) * self
    }
}

impl BlockStateProviderEvaluator {
    pub(crate) fn sample_block_state_provider_optional(
        level: &dyn LevelReader,
        registry: &Registry,
        random: &mut impl Random,
        provider: &BlockStateProviderKind,
        pos: BlockPos,
    ) -> Option<BlockStateId> {
        match provider {
            BlockStateProviderKind::Reference(provider) => {
                Self::sample_block_state_provider_optional(
                    level,
                    registry,
                    random,
                    &provider.kind,
                    pos,
                )
            }
            BlockStateProviderKind::RuleBased { fallback, rules } => {
                for rule in rules {
                    if Self::test_block_predicate(level, registry, &rule.if_true, pos)
                        && let Some(state) = Self::sample_block_state_provider_optional(
                            level, registry, random, &rule.then, pos,
                        )
                    {
                        return Some(state);
                    }
                }

                fallback.as_ref().and_then(|fallback| {
                    Self::sample_block_state_provider_optional(
                        level, registry, random, fallback, pos,
                    )
                })
            }
            BlockStateProviderKind::RandomBlock { blocks } => {
                Self::sample_random_block(registry, random, blocks)
            }
            _ => Some(Self::sample_block_state_provider(
                level, registry, random, provider, pos,
            )),
        }
    }

    pub(crate) fn sample_block_state_provider(
        level: &dyn LevelReader,
        registry: &Registry,
        random: &mut impl Random,
        provider: &BlockStateProviderKind,
        pos: BlockPos,
    ) -> BlockStateId {
        match provider {
            BlockStateProviderKind::Reference(provider) => {
                Self::sample_block_state_provider(level, registry, random, &provider.kind, pos)
            }
            BlockStateProviderKind::Simple { state } => {
                Self::block_state_from_data(registry, state)
            }
            BlockStateProviderKind::Weighted { entries } => {
                let total_weight = entries.iter().map(|entry| entry.weight).sum();
                let mut target = random.next_i32_bounded(total_weight);
                for entry in entries {
                    if target < entry.weight {
                        return Self::block_state_from_data(registry, &entry.data);
                    }
                    target -= entry.weight;
                }

                panic!("weighted blockstate provider failed to select an entry");
            }
            BlockStateProviderKind::RotatedBlock { state, direction } => {
                let direction = direction
                    .unwrap_or_else(|| Direction::ALL[random.next_i32_bounded(6) as usize]);
                let state = Self::sample_block_state_provider(level, registry, random, state, pos);
                let state = if state.try_get_value(&BlockStateProperties::AXIS).is_some() {
                    state.set_value(&BlockStateProperties::AXIS, direction.axis())
                } else {
                    state
                };

                let state = if state.try_get_value(&BlockStateProperties::FACING).is_some() {
                    state.set_value(&BlockStateProperties::FACING, direction)
                } else {
                    state
                };

                if direction.is_horizontal()
                    && state
                        .try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
                        .is_some()
                {
                    state.set_value(&BlockStateProperties::HORIZONTAL_FACING, direction)
                } else {
                    state
                }
            }
            BlockStateProviderKind::RandomizedInt {
                property,
                source,
                values,
            } => {
                let state = Self::sample_block_state_provider(level, registry, random, source, pos);
                let value = values.sample(random);

                Self::set_int_property_by_name(registry, state, property, value)
            }
            BlockStateProviderKind::RuleBased { .. } => {
                if let Some(state) = Self::sample_block_state_provider_optional(
                    level, registry, random, provider, pos,
                ) {
                    state
                } else {
                    level.get_block_state(pos)
                }
            }
            BlockStateProviderKind::Noise(provider) => {
                Self::sample_noise_provider(registry, provider, pos)
            }
            BlockStateProviderKind::NoiseThreshold(provider) => {
                Self::sample_noise_threshold_provider(registry, random, provider, pos)
            }
            BlockStateProviderKind::DualNoise(provider) => {
                Self::sample_dual_noise_provider(registry, provider, pos)
            }
            BlockStateProviderKind::CopyProperties { source } => {
                let sampled =
                    Self::sample_block_state_provider(level, registry, random, source, pos);

                registry
                    .blocks
                    .with_properties_of(sampled, level.get_block_state(pos))
            }
            BlockStateProviderKind::RandomBlock { blocks } => {
                Self::sample_random_block(registry, random, blocks)
                    .unwrap_or_else(|| level.get_block_state(pos))
            }
        }
    }

    fn sample_random_block(
        registry: &Registry,
        random: &mut impl Random,
        blocks: &BlockHolderSet,
    ) -> Option<BlockStateId> {
        let tagged_blocks: SmallVec<[BlockRef; 8]>;
        let entries = match blocks {
            BlockHolderSet::Entries(entries) => entries.as_slice(),
            BlockHolderSet::Tag(tag) => {
                tagged_blocks = registry.blocks.iter_tag(tag).collect();
                tagged_blocks.as_slice()
            }
        };
        if entries.is_empty() {
            return None;
        }
        let index = random.next_i32_bounded(entries.len() as i32) as usize;
        Some(registry.blocks.get_default_state_id(entries[index]))
    }

    pub(crate) fn set_int_property_by_name(
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

    pub(crate) fn sample_noise_provider(
        registry: &Registry,
        provider: &NoiseProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let noise = Self::normal_noise(&provider.noise, provider.seed);
        let noise_value = Self::noise_value(&noise, pos, provider.scale);
        Self::noise_state_by_value(registry, &provider.states, noise_value)
    }

    pub(crate) fn sample_noise_threshold_provider(
        registry: &Registry,
        random: &mut impl Random,
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

    pub(crate) fn sample_dual_noise_provider(
        registry: &Registry,
        provider: &DualNoiseProvider,
        pos: BlockPos,
    ) -> BlockStateId {
        let slow_noise = Self::normal_noise(&provider.slow_noise, provider.seed);
        let variety_noise = Self::noise_value(&slow_noise, pos, provider.slow_scale);
        let local_variety = map_clamped(
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

    pub(crate) fn normal_noise(parameters: &FeatureNoiseParameters, seed: i64) -> NormalNoise {
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(seed as u64));
        NormalNoise::create_from_random_with_params(
            &mut random,
            parameters.base_octave,
            parameters.base_amplitude,
            parameters.octave_count,
            parameters.normalize,
            &parameters.amplitude_modifiers,
        )
    }

    pub(crate) fn noise_value<S: NoiseScale>(noise: &NormalNoise, pos: BlockPos, scale: S) -> f64 {
        f64::from(noise.get_value(
            scale.scale_coord(pos.x()),
            scale.scale_coord(pos.y()),
            scale.scale_coord(pos.z()),
        ))
    }

    pub(crate) fn noise_state_by_value(
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

    pub(crate) fn noise_state_by_resolved_value(
        states: &[BlockStateId],
        noise_value: f64,
    ) -> BlockStateId {
        assert!(
            !states.is_empty(),
            "noise provider state list must not be empty"
        );
        states[Self::noise_state_index(states.len(), noise_value)]
    }

    pub(crate) fn noise_state_index(state_count: usize, noise_value: f64) -> usize {
        let placement_value = f32::midpoint(1.0_f32, noise_value as f32).clamp(0.0, 0.9999);
        (placement_value * state_count as f32) as usize
    }

    pub(crate) fn random_block_state_from_data_list(
        registry: &Registry,
        random: &mut impl Random,
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
}
