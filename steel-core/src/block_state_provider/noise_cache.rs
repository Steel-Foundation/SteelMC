use std::cell::RefCell;
use std::sync::Arc;

use steel_registry::feature::FeatureNoiseParameters;
use steel_worldgen::noise::NormalNoise;

use super::BlockStateProviderEvaluator;

// keep custom providers from growing a worker sampler cache indefinitely
const MAX_CACHED_NOISES: usize = 16;

thread_local! {
    static PROVIDER_NOISES: RefCell<NoiseCache> = const { RefCell::new(NoiseCache::new()) };
}

pub(super) struct NoiseCache {
    entries: Vec<CachedNoise>,
    next_eviction: usize,
}

struct CachedNoise {
    key: NoiseKey,
    noise: Arc<NormalNoise>,
}

struct NoiseKey {
    seed: i64,
    base_octave: i32,
    octave_count: i32,
    normalize: bool,
    base_amplitude_bits: u64,
    amplitude_modifier_bits: Box<[u64]>,
}

impl NoiseCache {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_eviction: 0,
        }
    }

    pub(super) fn get(parameters: &FeatureNoiseParameters, seed: i64) -> Arc<NormalNoise> {
        PROVIDER_NOISES.with_borrow_mut(|cache| cache.get_or_create(parameters, seed))
    }

    fn get_or_create(
        &mut self,
        parameters: &FeatureNoiseParameters,
        seed: i64,
    ) -> Arc<NormalNoise> {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.key.matches(parameters, seed))
        {
            return Arc::clone(&entry.noise);
        }

        let noise = Arc::new(BlockStateProviderEvaluator::normal_noise(parameters, seed));
        let entry = CachedNoise {
            key: NoiseKey::new(parameters, seed),
            noise: Arc::clone(&noise),
        };

        if self.entries.len() < MAX_CACHED_NOISES {
            self.entries.push(entry);
        } else {
            self.entries[self.next_eviction] = entry;
            self.next_eviction = (self.next_eviction + 1) % MAX_CACHED_NOISES;
        }

        noise
    }
}

impl NoiseKey {
    fn new(parameters: &FeatureNoiseParameters, seed: i64) -> Self {
        Self {
            seed,
            base_octave: parameters.base_octave,
            octave_count: parameters.octave_count,
            normalize: parameters.normalize,
            base_amplitude_bits: parameters.base_amplitude.to_bits(),
            amplitude_modifier_bits: parameters
                .amplitude_modifiers
                .iter()
                .map(|value| value.to_bits())
                .collect(),
        }
    }

    fn matches(&self, parameters: &FeatureNoiseParameters, seed: i64) -> bool {
        self.seed == seed
            && self.base_octave == parameters.base_octave
            && self.octave_count == parameters.octave_count
            && self.normalize == parameters.normalize
            && self.base_amplitude_bits == parameters.base_amplitude.to_bits()
            && self.amplitude_modifier_bits.iter().copied().eq(parameters
                .amplitude_modifiers
                .iter()
                .map(|value| value.to_bits()))
    }
}
