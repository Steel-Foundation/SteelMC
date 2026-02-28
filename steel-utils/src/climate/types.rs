//! Climate types for biome selection.

use std::cmp::Ordering;

use super::{PARAMETER_COUNT, QUANTIZATION_FACTOR, quantize_coord};

/// A target point representing sampled climate values.
///
/// All values are quantized (multiplied by 10000) to match vanilla's integer-based
/// distance calculations. This avoids floating-point precision issues in biome lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPoint {
    /// Temperature parameter
    pub temperature: i64,
    /// Humidity/vegetation parameter
    pub humidity: i64,
    /// Continentalness parameter (inland vs ocean)
    pub continentalness: i64,
    /// Erosion parameter
    pub erosion: i64,
    /// Depth parameter (surface vs underground)
    pub depth: i64,
    /// Weirdness/ridges parameter
    pub weirdness: i64,
}

impl TargetPoint {
    /// Create a new target point with quantized values.
    #[must_use]
    pub const fn new(
        temperature: i64,
        humidity: i64,
        continentalness: i64,
        erosion: i64,
        depth: i64,
        weirdness: i64,
    ) -> Self {
        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        }
    }

    /// Create a target point from f64 values (will be quantized).
    #[must_use]
    pub fn from_floats(
        temperature: f64,
        humidity: f64,
        continentalness: f64,
        erosion: f64,
        depth: f64,
        weirdness: f64,
    ) -> Self {
        Self {
            temperature: quantize_coord(temperature),
            humidity: quantize_coord(humidity),
            continentalness: quantize_coord(continentalness),
            erosion: quantize_coord(erosion),
            depth: quantize_coord(depth),
            weirdness: quantize_coord(weirdness),
        }
    }

    /// Convert to a 7-element array for tree lookups.
    /// The 7th element is always 0 (offset position).
    #[must_use]
    pub const fn to_parameter_array(&self) -> [i64; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            0, // Offset target is always 0
        ]
    }
}

/// A parameter range for biome matching.
///
/// Represents a range [min, max] that a climate parameter can match.
/// A point matches if it falls within this range; distance is 0 inside
/// and increases linearly outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parameter {
    /// Minimum value (quantized)
    pub min: i64,
    /// Maximum value (quantized)
    pub max: i64,
}

impl Parameter {
    /// Create a new parameter range.
    #[must_use]
    pub const fn new(min: i64, max: i64) -> Self {
        Self { min, max }
    }

    /// Create a point parameter (min == max).
    #[must_use]
    pub fn point(value: f32) -> Self {
        Self::span(value, value)
    }

    /// Create a parameter span from float values.
    #[must_use]
    pub fn span(min: f32, max: f32) -> Self {
        debug_assert!(min <= max, "min > max: {min} > {max}");
        Self {
            min: (min * QUANTIZATION_FACTOR) as i64,
            max: (max * QUANTIZATION_FACTOR) as i64,
        }
    }

    /// Create a parameter span from two parameters.
    #[must_use]
    pub const fn span_params(min: &Parameter, max: &Parameter) -> Self {
        debug_assert!(min.min <= max.max, "span_params: min > max");
        Self {
            min: min.min,
            max: max.max,
        }
    }

    /// Calculate the distance from a target value to this parameter range.
    ///
    /// Returns 0 if the target is within the range, otherwise the distance
    /// to the nearest edge.
    #[inline]
    #[must_use]
    pub const fn distance(&self, target: i64) -> i64 {
        let above = target - self.max;
        let below = self.min - target;
        if above > 0 {
            above
        } else if below > 0 {
            below
        } else {
            0
        }
    }

    /// Calculate the distance between two parameter ranges.
    #[inline]
    #[must_use]
    pub const fn distance_param(&self, target: &Parameter) -> i64 {
        let above = target.min - self.max;
        let below = self.min - target.max;
        if above > 0 {
            above
        } else if below > 0 {
            below
        } else {
            0
        }
    }

    /// Expand this parameter to include another parameter.
    #[must_use]
    pub const fn span_with(&self, other: Option<&Parameter>) -> Self {
        match other {
            Some(o) => Self {
                min: self.min.min(o.min),
                max: self.max.max(o.max),
            },
            None => *self,
        }
    }
}

/// A biome's full parameter specification.
///
/// Contains ranges for all 6 climate parameters plus an offset value
/// used as a tiebreaker in biome selection.
#[derive(Debug, Clone, Copy)]
pub struct ParameterPoint {
    /// Temperature range
    pub temperature: Parameter,
    /// Humidity range
    pub humidity: Parameter,
    /// Continentalness range
    pub continentalness: Parameter,
    /// Erosion range
    pub erosion: Parameter,
    /// Depth range
    pub depth: Parameter,
    /// Weirdness range
    pub weirdness: Parameter,
    /// Offset (quantized) - used as tiebreaker
    pub offset: i64,
}

impl ParameterPoint {
    /// Create a new parameter point.
    #[must_use]
    pub const fn new(
        temperature: Parameter,
        humidity: Parameter,
        continentalness: Parameter,
        erosion: Parameter,
        depth: Parameter,
        weirdness: Parameter,
        offset: i64,
    ) -> Self {
        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
            offset,
        }
    }

    /// Calculate the fitness (distance) between this parameter point and a target.
    ///
    /// Lower fitness = better match. Uses squared distances.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub const fn fitness(&self, target: &TargetPoint) -> i64 {
        let t = self.temperature.distance(target.temperature);
        let h = self.humidity.distance(target.humidity);
        let c = self.continentalness.distance(target.continentalness);
        let e = self.erosion.distance(target.erosion);
        let d = self.depth.distance(target.depth);
        let w = self.weirdness.distance(target.weirdness);

        // Sum of squared distances (matches vanilla Mth.square usage)
        t * t + h * h + c * c + e * e + d * d + w * w + self.offset * self.offset
    }

    /// Get the parameter space as a slice of parameters.
    #[must_use]
    pub const fn parameter_space(&self) -> [Parameter; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            Parameter::new(self.offset, self.offset),
        ]
    }
}

// =============================================================================
// R-Tree implementation matching vanilla's Climate.RTree
// =============================================================================

/// Maximum children per tree node. Matches vanilla's `CHILDREN_PER_NODE` = 6.
const CHILDREN_PER_NODE: usize = 6;

/// R-Tree node for spatial biome lookup.
enum RTreeNode {
    /// Leaf node containing a single biome entry.
    Leaf {
        parameter_space: [Parameter; PARAMETER_COUNT],
        value_index: usize,
    },
    /// Internal node with children and a bounding box.
    SubTree {
        parameter_space: [Parameter; PARAMETER_COUNT],
        children: Vec<RTreeNode>,
    },
}

impl RTreeNode {
    /// Get the parameter space (bounding box) of this node.
    const fn parameter_space(&self) -> &[Parameter; PARAMETER_COUNT] {
        match self {
            Self::Leaf {
                parameter_space, ..
            }
            | Self::SubTree {
                parameter_space, ..
            } => parameter_space,
        }
    }

    /// Calculate the minimum distance from a target point to this node's bounding box.
    fn distance(&self, target: &[i64; PARAMETER_COUNT]) -> i64 {
        let ps = self.parameter_space();
        let mut d = 0i64;
        for i in 0..PARAMETER_COUNT {
            let di = ps[i].distance(target[i]);
            d += di * di;
        }
        d
    }

    /// Search for the nearest leaf, returning (`value_index`, distance).
    ///
    /// Matches vanilla's `RTree.Node.search()` exactly, including
    /// the strict `>` pruning that affects tie-breaking.
    /// `best_dist` and `best_idx` represent the current best candidate
    /// (from lastResult caching or previous iteration).
    fn search(
        &self,
        target: &[i64; PARAMETER_COUNT],
        mut best_dist: i64,
        mut best_idx: Option<usize>,
    ) -> (Option<usize>, i64) {
        match self {
            Self::Leaf { value_index, .. } => {
                // Leaf always returns itself - the parent decides if it's better
                (Some(*value_index), self.distance(target))
            }
            Self::SubTree { children, .. } => {
                for child in children {
                    let child_dist = child.distance(target);
                    // Vanilla uses strict > for pruning (skips equal distance)
                    if best_dist > child_dist {
                        let (leaf_idx, leaf_dist) = child.search(target, best_dist, best_idx);
                        if best_dist > leaf_dist {
                            best_dist = leaf_dist;
                            best_idx = leaf_idx;
                        }
                    }
                }
                (best_idx, best_dist)
            }
        }
    }
}

/// Build the bounding box for a set of child nodes.
fn build_parameter_space(children: &[RTreeNode]) -> [Parameter; PARAMETER_COUNT] {
    let mut bounds: [Option<Parameter>; PARAMETER_COUNT] = [None; PARAMETER_COUNT];
    for child in children {
        let ps = child.parameter_space();
        for d in 0..PARAMETER_COUNT {
            bounds[d] = Some(ps[d].span_with(bounds[d].as_ref()));
        }
    }
    bounds.map(|b| b.expect("bounds should be initialized"))
}

/// Calculate the cost of a bounding box (sum of range widths).
fn cost(parameter_space: &[Parameter; PARAMETER_COUNT]) -> i64 {
    let mut result = 0i64;
    for p in parameter_space {
        result += (p.max - p.min).abs();
    }
    result
}

/// Build data used during tree construction. Holds the parameter space
/// and original indices for sorting.
struct BuildEntry {
    parameter_space: [Parameter; PARAMETER_COUNT],
    index: usize,
}

/// Build an R-Tree from a list of entries, matching vanilla's algorithm.
#[allow(clippy::needless_range_loop, clippy::too_many_lines)]
fn build_tree(entries: &mut [BuildEntry]) -> RTreeNode {
    assert!(!entries.is_empty());

    if entries.len() == 1 {
        return RTreeNode::Leaf {
            parameter_space: entries[0].parameter_space,
            value_index: entries[0].index,
        };
    }

    if entries.len() <= CHILDREN_PER_NODE {
        // Sort by total magnitude of centers across all dimensions
        entries.sort_by_key(|e| {
            let mut total: i64 = 0;
            for d in 0..PARAMETER_COUNT {
                let p = &e.parameter_space[d];
                total += i64::midpoint(p.min, p.max).abs();
            }
            total
        });

        let children: Vec<RTreeNode> = entries
            .iter()
            .map(|e| RTreeNode::Leaf {
                parameter_space: e.parameter_space,
                value_index: e.index,
            })
            .collect();
        let ps = build_parameter_space(&children);
        return RTreeNode::SubTree {
            parameter_space: ps,
            children,
        };
    }

    // Try splitting along each dimension, choose minimum cost
    let mut min_cost = i64::MAX;
    let mut best_dim = 0;

    for d in 0..PARAMETER_COUNT {
        sort_entries(entries, d);
        let bucket_cost = compute_bucket_cost(entries);
        if min_cost > bucket_cost {
            min_cost = bucket_cost;
            best_dim = d;
        }
    }

    // Sort by the best dimension and bucketize
    sort_entries(entries, best_dim);
    let bucket_ranges = compute_bucket_ranges(entries.len());

    // Build subtrees for each bucket
    let mut bucket_subtrees: Vec<(RTreeNode, [Parameter; PARAMETER_COUNT])> = Vec::new();
    for (start, end) in &bucket_ranges {
        let bucket_entries = &entries[*start..*end];
        let ps = {
            let mut bounds: [Option<Parameter>; PARAMETER_COUNT] = [None; PARAMETER_COUNT];
            for e in bucket_entries {
                for dim in 0..PARAMETER_COUNT {
                    bounds[dim] = Some(e.parameter_space[dim].span_with(bounds[dim].as_ref()));
                }
            }
            bounds.map(|b| b.expect("bounds should be initialized"))
        };
        bucket_subtrees.push((
            RTreeNode::SubTree {
                parameter_space: ps,
                children: bucket_entries
                    .iter()
                    .map(|e| RTreeNode::Leaf {
                        parameter_space: e.parameter_space,
                        value_index: e.index,
                    })
                    .collect(),
            },
            ps,
        ));
    }

    // Sort the bucket subtrees by the best dimension (absolute=true)
    sort_subtrees(&mut bucket_subtrees, best_dim);

    // For each bucket subtree, take its children and recursively build
    let mut final_children: Vec<RTreeNode> = Vec::new();
    for (subtree, _) in bucket_subtrees {
        match subtree {
            RTreeNode::SubTree { children, .. } => {
                // Convert children back to BuildEntry for recursive build
                let mut child_entries: Vec<BuildEntry> = children
                    .into_iter()
                    .map(|node| {
                        let ps = *node.parameter_space();
                        let idx = match &node {
                            RTreeNode::Leaf { value_index, .. } => *value_index,
                            RTreeNode::SubTree { .. } => unreachable!(),
                        };
                        BuildEntry {
                            parameter_space: ps,
                            index: idx,
                        }
                    })
                    .collect();
                final_children.push(build_tree(&mut child_entries));
            }
            RTreeNode::Leaf { .. } => unreachable!(),
        }
    }

    let ps = build_parameter_space(&final_children);
    RTreeNode::SubTree {
        parameter_space: ps,
        children: final_children,
    }
}

/// Sort entries by a dimension, with tiebreaking by subsequent dimensions.
fn sort_entries(entries: &mut [BuildEntry], dimension: usize) {
    entries.sort_by(|a, b| {
        for offset in 0..PARAMETER_COUNT {
            let d = (dimension + offset) % PARAMETER_COUNT;
            let center_a = i64::midpoint(a.parameter_space[d].min, a.parameter_space[d].max);
            let center_b = i64::midpoint(b.parameter_space[d].min, b.parameter_space[d].max);
            let cmp = center_a.cmp(&center_b);
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
        Ordering::Equal
    });
}

/// Sort bucket subtrees by a dimension (absolute=true).
fn sort_subtrees(subtrees: &mut [(RTreeNode, [Parameter; PARAMETER_COUNT])], dimension: usize) {
    subtrees.sort_by(|a, b| {
        for offset in 0..PARAMETER_COUNT {
            let d = (dimension + offset) % PARAMETER_COUNT;
            let center_a = i64::midpoint(a.1[d].min, a.1[d].max);
            let center_b = i64::midpoint(b.1[d].min, b.1[d].max);
            let cmp = center_a.abs().cmp(&center_b.abs());
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
        Ordering::Equal
    });
}

/// Compute the expected bucket size from vanilla's formula.
fn expected_children_count(total: usize) -> usize {
    let log_base_6 = ((total as f64) - 0.01).ln() / (CHILDREN_PER_NODE as f64).ln();
    (CHILDREN_PER_NODE as f64).powf(log_base_6.floor()) as usize
}

/// Compute bucket index ranges for a list of entries.
fn compute_bucket_ranges(total: usize) -> Vec<(usize, usize)> {
    let expected = expected_children_count(total);
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < total {
        let end = (start + expected).min(total);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// Compute the total cost of bucketing entries.
#[allow(clippy::needless_range_loop)]
fn compute_bucket_cost(entries: &[BuildEntry]) -> i64 {
    let ranges = compute_bucket_ranges(entries.len());
    let mut total_cost = 0i64;
    for (start, end) in ranges {
        let mut bounds: [Option<Parameter>; PARAMETER_COUNT] = [None; PARAMETER_COUNT];
        for e in &entries[start..end] {
            for d in 0..PARAMETER_COUNT {
                bounds[d] = Some(e.parameter_space[d].span_with(bounds[d].as_ref()));
            }
        }
        let ps = bounds.map(|b| b.expect("bounds should be initialized"));
        total_cost += cost(&ps);
    }
    total_cost
}

/// A list of biome parameter points with their associated values.
///
/// Uses an R-Tree for lookup matching vanilla's `Climate.ParameterList`.
pub struct ParameterList<T> {
    /// The biome entries (parameter point, value pairs)
    values: Vec<(ParameterPoint, T)>,
    /// Cached parameter spaces for each value (for distance computation in lastResult)
    param_spaces: Vec<[Parameter; PARAMETER_COUNT]>,
    /// R-Tree root for efficient spatial lookup
    root: RTreeNode,
}

impl<T> ParameterList<T> {
    /// Create a new parameter list from values, building an R-Tree index.
    ///
    /// # Panics
    ///
    /// Panics if `values` is empty.
    #[must_use]
    pub fn new(values: Vec<(ParameterPoint, T)>) -> Self {
        assert!(!values.is_empty(), "Need at least one value");

        let param_spaces: Vec<[Parameter; PARAMETER_COUNT]> =
            values.iter().map(|(pp, _)| pp.parameter_space()).collect();

        // Build R-Tree from the parameter points
        let mut entries: Vec<BuildEntry> = values
            .iter()
            .enumerate()
            .map(|(i, (pp, _))| BuildEntry {
                parameter_space: pp.parameter_space(),
                index: i,
            })
            .collect();

        let root = build_tree(&mut entries);

        Self {
            values,
            param_spaces,
            root,
        }
    }

    /// Get the underlying values.
    #[must_use]
    pub fn values(&self) -> &[(ParameterPoint, T)] {
        &self.values
    }

    /// Find the best matching value for a target point (no caching).
    ///
    /// Uses R-Tree search matching vanilla's `Climate.ParameterList.findValue()`.
    ///
    /// Note: Vanilla warm-starts with `lastResult` via `ThreadLocal`, which can
    /// affect tie-breaking on equal-distance candidates. This version starts
    /// from `i64::MAX` (no warm-start). Use `find_value_cached` for the hot
    /// path to match vanilla's tie-breaking behavior.
    ///
    /// # Panics
    ///
    /// Panics if the R-Tree search fails to find any matching value.
    #[must_use]
    pub fn find_value(&self, target: &TargetPoint) -> &T {
        let target_array = target.to_parameter_array();
        let (idx, _) = self.root.search(&target_array, i64::MAX, None);
        &self.values[idx.expect("R-Tree search should always find a value")].1
    }

    /// Find the best matching value with lastResult caching.
    ///
    /// Matches vanilla's `Climate.ParameterList.findValue()` with `ThreadLocal`
    /// `lastNode` warm-starting. The cache stores the index of the last result,
    /// which is used as the initial candidate for the next search, improving
    /// both performance and tie-breaking behavior.
    ///
    /// # Panics
    ///
    /// Panics if the R-Tree search fails to find any matching value.
    #[must_use]
    pub fn find_value_cached(&self, target: &TargetPoint, cache: &mut Option<usize>) -> &T {
        let target_array = target.to_parameter_array();

        // Compute initial distance from cached last result
        let (init_dist, init_idx) = match *cache {
            Some(idx) => {
                let ps = &self.param_spaces[idx];
                let mut d = 0i64;
                for i in 0..PARAMETER_COUNT {
                    let di = ps[i].distance(target_array[i]);
                    d += di * di;
                }
                (d, Some(idx))
            }
            None => (i64::MAX, None),
        };

        let (result_idx, _) = self.root.search(&target_array, init_dist, init_idx);
        let result_idx = result_idx.expect("R-Tree search should always find a value");
        *cache = Some(result_idx);
        &self.values[result_idx].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_point_from_floats() {
        let target = TargetPoint::from_floats(0.5, -0.3, 0.0, 0.1, 0.0, 0.2);
        assert_eq!(target.temperature, 5000);
        assert_eq!(target.humidity, -3000);
        assert_eq!(target.continentalness, 0);
        assert_eq!(target.erosion, 1000);
        assert_eq!(target.depth, 0);
        assert_eq!(target.weirdness, 2000);
    }

    #[test]
    fn test_parameter_distance() {
        let param = Parameter::new(-5000, 5000);

        // Inside range
        assert_eq!(param.distance(0), 0);
        assert_eq!(param.distance(5000), 0);
        assert_eq!(param.distance(-5000), 0);

        // Outside range
        assert_eq!(param.distance(6000), 1000);
        assert_eq!(param.distance(-6000), 1000);
        assert_eq!(param.distance(10000), 5000);
    }

    #[test]
    fn test_parameter_point_fitness() {
        let params = ParameterPoint::new(
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            0,
        );

        // Perfect match
        let target = TargetPoint::new(0, 0, 0, 0, 0, 0);
        assert_eq!(params.fitness(&target), 0);

        // Off by 100 in temperature
        let target = TargetPoint::new(100, 0, 0, 0, 0, 0);
        assert_eq!(params.fitness(&target), 100 * 100);

        // Off by 100 in two parameters
        let target = TargetPoint::new(100, 100, 0, 0, 0, 0);
        assert_eq!(params.fitness(&target), 100 * 100 + 100 * 100);
    }

    #[test]
    fn test_parameter_list_find_value() {
        let values = vec![
            (
                ParameterPoint::new(
                    Parameter::new(-10000, 0),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    0,
                ),
                "cold",
            ),
            (
                ParameterPoint::new(
                    Parameter::new(0, 10000),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    Parameter::new(0, 0),
                    0,
                ),
                "hot",
            ),
        ];

        let list = ParameterList::new(values);

        // Cold biome should match negative temperature
        let cold_target = TargetPoint::new(-5000, 0, 0, 0, 0, 0);
        assert_eq!(*list.find_value(&cold_target), "cold");

        // Hot biome should match positive temperature
        let hot_target = TargetPoint::new(5000, 0, 0, 0, 0, 0);
        assert_eq!(*list.find_value(&hot_target), "hot");
    }
}
