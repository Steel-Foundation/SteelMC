//! R-Tree backed parameter list for climate-based biome lookup.
//!
//! Implements vanilla's `Climate.ParameterList` with a flattened R-Tree for
//! cache-efficient nearest-neighbor search. The tree is built once at startup
//! using vanilla's algorithm, then flattened into a BFS-ordered contiguous
//! array where children of the same parent occupy adjacent indices.

use std::cmp::Ordering;

use super::PARAMETER_COUNT;
use super::types::{Parameter, ParameterPoint, TargetPoint};

// =============================================================================
// R-Tree construction (vanilla's Climate.RTree algorithm)
// =============================================================================

/// Maximum children per tree node. Matches vanilla's `CHILDREN_PER_NODE` = 6.
const CHILDREN_PER_NODE: usize = 6;

/// R-Tree node used during construction only. After building, the tree is
/// flattened into a `Vec<FlatNode>` for search.
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
}

/// Build data used during tree construction.
struct BuildEntry {
    parameter_space: [Parameter; PARAMETER_COUNT],
    index: usize,
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

// =============================================================================
// Flat R-Tree: cache-friendly contiguous layout
// =============================================================================

/// Compact node for the flattened R-Tree.
///
/// Children of the same parent are stored at contiguous indices in a single
/// `Vec<FlatNode>`, enabling cache-efficient iteration during search.
/// The BFS-order layout also means that nodes accessed together during a
/// search tend to be near each other in memory.
struct FlatNode {
    /// Bounding box minimum values for each parameter dimension.
    mins: [i64; PARAMETER_COUNT],
    /// Bounding box maximum values for each parameter dimension.
    maxs: [i64; PARAMETER_COUNT],
    /// For leaf nodes: index into the values array.
    /// For subtree nodes: `u32::MAX` (sentinel).
    value_index: u32,
    /// Start index of children in the nodes array (subtree only).
    children_start: u32,
    /// Number of children (0 = leaf, 1..=6 = subtree).
    children_count: u8,
}

impl FlatNode {
    /// Compute the squared distance from a target point to this node's bounding box.
    ///
    /// Uses branchless `max` operations (compiles to `cmov`) to avoid branch
    /// mispredictions in the hot inner loop.
    #[inline]
    #[allow(clippy::needless_range_loop)] // Indexing into 3 parallel arrays; iterator would be less clear
    fn distance(&self, target: &[i64; PARAMETER_COUNT]) -> i64 {
        let mut d = 0i64;
        for i in 0..PARAMETER_COUNT {
            let di = (target[i] - self.maxs[i])
                .max(self.mins[i] - target[i])
                .max(0);
            d += di * di;
        }
        d
    }

    #[inline]
    const fn is_leaf(&self) -> bool {
        self.children_count == 0
    }
}

/// Flatten an R-Tree into a contiguous `Vec` using BFS ordering.
///
/// BFS guarantees that all children of the same parent occupy contiguous
/// indices, which is the key property for cache-efficient search.
fn flatten_tree(root: RTreeNode) -> Vec<FlatNode> {
    use std::collections::VecDeque;

    let mut nodes: Vec<FlatNode> = Vec::new();
    // Queue of (children_batch, parent_flat_index).
    // Each batch is a Vec of siblings that will be laid out contiguously.
    let mut queue: VecDeque<(Vec<RTreeNode>, Option<u32>)> = VecDeque::new();
    queue.push_back((vec![root], None));

    while let Some((batch, parent_idx)) = queue.pop_front() {
        let batch_start = nodes.len() as u32;

        // Fix up parent's children_start to point to this batch
        if let Some(pidx) = parent_idx {
            nodes[pidx as usize].children_start = batch_start;
        }

        for node in batch {
            let flat_idx = nodes.len() as u32;
            match node {
                RTreeNode::Leaf {
                    parameter_space,
                    value_index,
                } => {
                    nodes.push(FlatNode {
                        mins: parameter_space.map(|p| p.min),
                        maxs: parameter_space.map(|p| p.max),
                        value_index: value_index as u32,
                        children_start: 0,
                        children_count: 0,
                    });
                }
                RTreeNode::SubTree {
                    parameter_space,
                    children,
                } => {
                    let children_count = children.len() as u8;
                    nodes.push(FlatNode {
                        mins: parameter_space.map(|p| p.min),
                        maxs: parameter_space.map(|p| p.max),
                        value_index: u32::MAX,
                        children_start: 0, // fixed up when children batch is processed
                        children_count,
                    });
                    queue.push_back((children, Some(flat_idx)));
                }
            }
        }
    }

    nodes
}

/// Search the flat R-Tree for the nearest leaf to the target.
///
/// When a child passes the bounding-box distance test and is a leaf, the
/// distance is used directly instead of recursing, avoiding redundant
/// recomputation. Vanilla's strict `>` pruning semantics are preserved.
fn search_nearest(
    nodes: &[FlatNode],
    node: &FlatNode,
    target: &[i64; PARAMETER_COUNT],
    best_dist: &mut i64,
    best_idx: &mut Option<u32>,
) {
    let start = node.children_start as usize;
    let end = start + node.children_count as usize;
    let children = &nodes[start..end];

    for child in children {
        let child_dist = child.distance(target);
        // Vanilla uses strict > for pruning (skips equal distance)
        if *best_dist > child_dist {
            if child.is_leaf() {
                // Leaf: child_dist IS the exact distance — no recursion needed
                *best_dist = child_dist;
                *best_idx = Some(child.value_index);
            } else {
                // Subtree: recurse into children
                search_nearest(nodes, child, target, best_dist, best_idx);
            }
        }
    }
}

// =============================================================================
// Public API
// =============================================================================

/// A list of biome parameter points with their associated values.
///
/// Uses an R-Tree for lookup matching vanilla's `Climate.ParameterList`.
pub struct ParameterList<T> {
    /// The biome entries (parameter point, value pairs)
    values: Vec<(ParameterPoint, T)>,
    /// Cached parameter spaces for each value (for distance computation in lastResult)
    param_spaces: Vec<[Parameter; PARAMETER_COUNT]>,
    /// Flat R-Tree nodes in BFS order. Root is at index 0.
    nodes: Vec<FlatNode>,
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
        let nodes = flatten_tree(root);

        Self {
            values,
            param_spaces,
            nodes,
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
        let root = &self.nodes[0];
        if root.is_leaf() {
            return &self.values[root.value_index as usize].1;
        }
        let mut best_dist = i64::MAX;
        let mut best_idx = None;
        search_nearest(
            &self.nodes,
            root,
            &target_array,
            &mut best_dist,
            &mut best_idx,
        );
        &self.values[best_idx.expect("R-Tree search should always find a value") as usize].1
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

        let root = &self.nodes[0];
        if root.is_leaf() {
            let idx = root.value_index as usize;
            *cache = Some(idx);
            return &self.values[idx].1;
        }

        // Compute initial distance from cached last result
        let (mut best_dist, init_idx) = match *cache {
            Some(idx) => {
                let ps = &self.param_spaces[idx];
                let mut d = 0i64;
                for i in 0..PARAMETER_COUNT {
                    let di = ps[i].distance(target_array[i]);
                    d += di * di;
                }
                (d, Some(idx as u32))
            }
            None => (i64::MAX, None),
        };

        let mut best_idx = init_idx;
        search_nearest(
            &self.nodes,
            root,
            &target_array,
            &mut best_dist,
            &mut best_idx,
        );
        let result_idx = best_idx.expect("R-Tree search should always find a value") as usize;
        *cache = Some(result_idx);
        &self.values[result_idx].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
