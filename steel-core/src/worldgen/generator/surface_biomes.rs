//! Fuzzed biome lookups for the surface stage.

use glam::IVec3;
use rustc_hash::FxHashMap;
use steel_registry::REGISTRY;
use steel_registry::biome::TemperatureModifier;
use steel_worldgen::density::{DimensionNoises, NoiseSettings};
use steel_worldgen::surface::SurfaceBiomeProvider;
use steel_worldgen::surface_partial::PartialSurfaceOutcome;

use super::vanilla::{get_fiddle, lcg_next};

/// Partition of biome ids into classes.
pub(super) struct SurfaceBiomeClasses {
    class_of: Box<[u16]>,
    /// Rule outcome below the preliminary surface, `[class][y - min_y]`;
    /// empty when the rule does not use the preliminary surface.
    below_preliminary: Box<[PartialSurfaceOutcome]>,
    min_y: i32,
    height: i32,
}

impl SurfaceBiomeClasses {
    /// Classes for `N`'s surface rule: equal `biome_is` memberships, base
    /// temperature and temperature modifier. `None` when the rule reads no
    /// biome or tests more sets than the membership mask holds.
    pub(super) fn new<N: DimensionNoises>() -> Option<Self> {
        let sets = N::surface_rule_biome_sets();
        if !N::surface_rule_uses_biome() || sets.len() > u64::BITS as usize {
            return None;
        }

        let mut classes = FxHashMap::<(u64, u32, bool), u16>::default();
        let mut class_masks = Vec::new();
        let class_of = REGISTRY
            .biomes
            .iter()
            .map(|(id, biome)| {
                let mask = sets
                    .iter()
                    .enumerate()
                    .filter(|(_, set)| set.contains(&(id as u16)))
                    .fold(0u64, |mask, (bit, _)| mask | 1 << bit);
                let key = (
                    mask,
                    biome.temperature.to_bits(),
                    matches!(biome.temperature_modifier, TemperatureModifier::Frozen),
                );
                let next = classes.len() as u16;
                *classes.entry(key).or_insert_with(|| {
                    class_masks.push(mask);
                    next
                })
            })
            .collect();

        let min_y = N::Settings::MIN_Y;
        let height = N::Settings::HEIGHT;
        let below_preliminary = if N::surface_rule_uses_preliminary_surface() {
            let rule = N::surface_rule_below_preliminary_surface();
            class_masks
                .iter()
                .flat_map(|&mask| (min_y..min_y + height).map(move |y| rule.resolve(y, mask)))
                .collect()
        } else {
            Box::default()
        };
        Some(Self {
            class_of,
            below_preliminary,
            min_y,
            height,
        })
    }

    fn class(&self, biome_id: u16) -> Option<u16> {
        self.class_of.get(usize::from(biome_id)).copied()
    }

    fn below_preliminary_outcome(&self, class: u16, block_y: i32) -> PartialSurfaceOutcome {
        let index = usize::from(class) * self.height as usize + (block_y - self.min_y) as usize;
        self.below_preliminary
            .get(index)
            .copied()
            .unwrap_or(PartialSurfaceOutcome::Evaluate)
    }
}

/// Quart biome reads for one chunk and the one-quart ring around it.
pub(super) struct SurfaceQuartBiomes<'a> {
    biome_data: &'a [u16],
    chunk_quart_x: i32,
    chunk_quart_z: i32,
    min_qy: i32,
    total_quarts_y: i32,
    neighbor_biomes: &'a dyn Fn(IVec3) -> u16,
    ring: Box<[u16]>,
}

const RING_SIDE: i32 = 6;
const UNREAD: u16 = u16::MAX;

impl<'a> SurfaceQuartBiomes<'a> {
    pub(super) fn new(
        biome_data: &'a [u16],
        section_count: usize,
        min_y: i32,
        chunk_quart_x: i32,
        chunk_quart_z: i32,
        neighbor_biomes: &'a dyn Fn(IVec3) -> u16,
    ) -> Self {
        let total_quarts_y = (section_count * 4) as i32;
        Self {
            biome_data,
            chunk_quart_x,
            chunk_quart_z,
            min_qy: min_y >> 2,
            total_quarts_y,
            neighbor_biomes,
            ring: vec![UNREAD; (RING_SIDE * RING_SIDE * total_quarts_y) as usize].into(),
        }
    }

    #[expect(
        clippy::similar_names,
        reason = "quart coordinate names mirror the x/y/z axes"
    )]
    fn biome_at(&mut self, quart: IVec3) -> u16 {
        let qy = (quart.y - self.min_qy).clamp(0, self.total_quarts_y - 1);
        let local_qx = quart.x - self.chunk_quart_x;
        let local_qz = quart.z - self.chunk_quart_z;

        if (0..4).contains(&local_qx) && (0..4).contains(&local_qz) {
            let (section_idx, local_qy) = (qy / 4, qy % 4);
            return self.biome_data
                [(section_idx * 64 + local_qy * 16 + local_qz * 4 + local_qx) as usize];
        }

        let (rx, rz) = (local_qx + 1, local_qz + 1);
        if !(0..RING_SIDE).contains(&rx) || !(0..RING_SIDE).contains(&rz) {
            return (self.neighbor_biomes)(quart);
        }
        let slot = &mut self.ring[((qy * RING_SIDE + rz) * RING_SIDE + rx) as usize];
        if *slot == UNREAD {
            *slot = (self.neighbor_biomes)(quart);
        }
        *slot
    }
}

/// Column-local cache for fuzzed biome lookups (vanilla `BiomeManager.getBiome()`).
pub(super) struct FuzzedBiomeColumn<'a, 'b> {
    quarts: &'a mut SurfaceQuartBiomes<'b>,
    classes: Option<&'a SurfaceBiomeClasses>,
    biome_zoom_seed: i64,
    parent_x: i32,
    parent_z: i32,
    fract_x: f64,
    fract_z: f64,
    cached_parent_y: i32,
    candidates: [(f64, f64); 8],
    rval_after_cx: [i64; 2],
    class_parent_y: i32,
    uniform_biome: Option<u16>,
}

impl<'a, 'b> FuzzedBiomeColumn<'a, 'b> {
    pub(super) fn new(
        quarts: &'a mut SurfaceQuartBiomes<'b>,
        classes: Option<&'a SurfaceBiomeClasses>,
        biome_zoom_seed: i64,
        block_x: i32,
        block_z: i32,
    ) -> Self {
        let abs_x = block_x - 2;
        let abs_z = block_z - 2;
        let parent_x = abs_x >> 2;
        let parent_z = abs_z >> 2;
        Self {
            quarts,
            classes,
            biome_zoom_seed,
            parent_x,
            parent_z,
            fract_x: f64::from(abs_x & 3) / 4.0,
            fract_z: f64::from(abs_z & 3) / 4.0,
            cached_parent_y: i32::MIN,
            candidates: [(0.0, 0.0); 8],
            rval_after_cx: [
                lcg_next(biome_zoom_seed, i64::from(parent_x)),
                lcg_next(biome_zoom_seed, i64::from(parent_x + 1)),
            ],
            class_parent_y: i32::MIN,
            uniform_biome: None,
        }
    }

    /// Compute candidates for a given `cy`, writing to either the low (bit1=0)
    /// or high (bit1=1) slots.
    #[inline]
    fn compute_cy_group(&mut self, cy: i32, high: bool) {
        let base_idx = if high { 2 } else { 0 };
        for cx_idx in 0..2usize {
            let cx = self.parent_x + cx_idx as i32;
            let dx = if cx_idx == 0 {
                self.fract_x
            } else {
                self.fract_x - 1.0
            };
            let rval_cy = lcg_next(self.rval_after_cx[cx_idx], i64::from(cy));
            for cz_off in 0..2usize {
                let cz = self.parent_z + cz_off as i32;
                let dz = if cz_off == 0 {
                    self.fract_z
                } else {
                    self.fract_z - 1.0
                };

                let mut rval = lcg_next(rval_cy, i64::from(cz));
                rval = lcg_next(rval, i64::from(cx));
                rval = lcg_next(rval, i64::from(cy));
                rval = lcg_next(rval, i64::from(cz));
                let fx = get_fiddle(rval);
                rval = lcg_next(rval, self.biome_zoom_seed);
                let fy = get_fiddle(rval);
                rval = lcg_next(rval, self.biome_zoom_seed);
                let fz = get_fiddle(rval);

                let xz_partial = (dx + fx) * (dx + fx) + (dz + fz) * (dz + fz);
                self.candidates[cx_idx * 4 + base_idx + cz_off] = (fy, xz_partial);
            }
        }
    }

    /// Recompute the 8 candidate fiddle values and X/Z distance for a new `parent_y`.
    fn recompute_candidates(&mut self, parent_y: i32) {
        if self.cached_parent_y != i32::MIN && parent_y == self.cached_parent_y - 1 {
            // Reuse: old low-cy group → new high-cy group
            self.candidates[2] = self.candidates[0];
            self.candidates[3] = self.candidates[1];
            self.candidates[6] = self.candidates[4];
            self.candidates[7] = self.candidates[5];
            self.compute_cy_group(parent_y, false);
        } else {
            self.compute_cy_group(parent_y, false);
            self.compute_cy_group(parent_y + 1, true);
        }
        self.cached_parent_y = parent_y;
    }

    /// Exact fuzzed biome for a given `block_y` (vanilla `BiomeManager.getBiome`).
    #[inline]
    pub(super) fn get(&mut self, block_y: i32) -> u16 {
        let abs_y = block_y - 2;
        let parent_y = abs_y >> 2;
        let fract_y = f64::from(abs_y & 3) / 4.0;

        if parent_y != self.cached_parent_y {
            self.recompute_candidates(parent_y);
        }

        let mut min_i = 0usize;
        let mut min_dist = f64::INFINITY;
        for i in 0..8usize {
            let (fy, xz_partial) = self.candidates[i];
            let dy = if (i & 2) == 0 { fract_y } else { fract_y - 1.0 };
            let dist = xz_partial + (dy + fy) * (dy + fy);
            if min_dist > dist {
                min_i = i;
                min_dist = dist;
            }
        }

        self.quarts.biome_at(IVec3::new(
            self.parent_x + i32::from(min_i & 4 != 0),
            parent_y + i32::from(min_i & 2 != 0),
            self.parent_z + i32::from(min_i & 1 != 0),
        ))
    }

    /// A biome the surface rule cannot distinguish from [`Self::get`]'s.
    pub(super) fn get_for_surface_rule(&mut self, block_y: i32) -> u16 {
        match self.uniform_group_biome(block_y) {
            Some(biome) => biome,
            None => self.get(block_y),
        }
    }

    /// What the surface rule does to a default block at `block_y`, known to be
    /// below the column's preliminary surface, when decidable from Y and the
    /// biome class of a uniform candidate group alone.
    pub(super) fn below_preliminary_outcome(&mut self, block_y: i32) -> PartialSurfaceOutcome {
        let Some(classes) = self.classes else {
            return PartialSurfaceOutcome::Evaluate;
        };
        self.uniform_group_biome(block_y)
            .and_then(|biome| classes.class(biome))
            .map_or(PartialSurfaceOutcome::Evaluate, |class| {
                classes.below_preliminary_outcome(class, block_y)
            })
    }

    /// The group's shared-class candidate biome, if its 8 candidates agree.
    fn uniform_group_biome(&mut self, block_y: i32) -> Option<u16> {
        let classes = self.classes?;
        let parent_y = (block_y - 2) >> 2;
        if parent_y != self.class_parent_y {
            self.class_parent_y = parent_y;
            self.uniform_biome = self.uniform_candidate_biome(classes, parent_y);
        }
        self.uniform_biome
    }

    fn uniform_candidate_biome(
        &mut self,
        classes: &SurfaceBiomeClasses,
        parent_y: i32,
    ) -> Option<u16> {
        let first = self
            .quarts
            .biome_at(IVec3::new(self.parent_x, parent_y, self.parent_z));
        let class = classes.class(first)?;
        for i in 1..8 {
            let biome = self.quarts.biome_at(IVec3::new(
                self.parent_x + i32::from(i & 4 != 0),
                parent_y + i32::from(i & 2 != 0),
                self.parent_z + i32::from(i & 1 != 0),
            ));
            if classes.class(biome) != Some(class) {
                return None;
            }
        }
        Some(first)
    }
}

impl SurfaceBiomeProvider for FuzzedBiomeColumn<'_, '_> {
    #[inline]
    fn biome_id(&mut self, block_y: i32) -> u16 {
        self.get_for_surface_rule(block_y)
    }
}
