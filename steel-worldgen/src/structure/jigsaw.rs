//! Jigsaw assembly. Ports vanilla's `JigsawPlacement` BFS: connects pieces via
//! jigsaw blocks given a start pool + config. Produces typed piece state;
//! block placement runs in a later worldgen stage.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::{array, mem, ptr};

use glam::IVec3;
use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::structure::{
    JigsawConfig, LiquidSettingsData, PoolAlias, StartHeight, StructureData,
};
use steel_registry::template_pool::{
    JigsawOrientation, JointType, PoolElement, Projection, TemplateData, TemplatePoolData,
};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{PositionalRandom, Random};
use steel_utils::{BoundingBox, Identifier, Rotation};

use crate::structure::box_octree::BoxOctree;
use crate::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece, StructurePiecePayload,
};

/// A placed piece produced by jigsaw assembly.
#[derive(Debug, Clone)]
pub struct PlacedPiece {
    /// Selected pool element.
    pub element: PoolElement,
    /// Template location (Single/LegacySingle).
    pub template_location: Option<Identifier>,
    /// World-space origin.
    pub position: IVec3,
    /// Rotation.
    pub rotation: Rotation,
    /// Template-sized BB (used for beardifier + world save).
    pub bounding_box: BoundingBox,
    /// Assembly-time BB, possibly expanded vertically by the expansion hack.
    /// Used only during assembly — not persisted.
    pub assembly_bb: BoundingBox,
    /// Ground-level delta for Beardifier.
    pub ground_level_delta: i32,
    /// Rigid or terrain-matching.
    pub projection: Projection,
    /// BFS tree depth.
    pub depth: i32,
    /// Junctions to neighbors.
    pub junctions: Vec<JigsawJunction>,
}

/// Typed state needed to place or compare a vanilla jigsaw piece.
#[derive(Debug, Clone)]
pub struct JigsawPieceData {
    /// Selected pool element.
    pub pool_element: PoolElement,
    /// World-space template origin.
    pub position: IVec3,
    /// Template rotation.
    pub rotation: Rotation,
    /// Liquid handling mode for block placement.
    pub liquid_settings: LiquidSettingsData,
}

/// Junction between two jigsaw pieces (terrain adaptation).
#[derive(Debug, Clone)]
pub struct JigsawJunction {
    /// World-space source position.
    pub source_pos: IVec3,
    /// Y delta between source and target.
    pub delta_y: i32,
    /// Destination projection.
    pub dest_projection: Projection,
}

/// Resolves pool aliases for a specific structure instance.
pub fn resolve_aliases(
    aliases: &[PoolAlias],
    rng: &mut impl Random,
) -> FxHashMap<Identifier, Identifier> {
    let mut map = FxHashMap::default();
    for alias in aliases {
        match alias {
            PoolAlias::Direct { alias, target } => {
                map.insert(alias.clone(), target.clone());
            }
            PoolAlias::Random { alias, targets } => {
                let total: i32 = targets.iter().map(|(_, w)| *w).sum();
                if total > 0 {
                    let mut pick = rng.next_i32_bounded(total);
                    for (target, weight) in targets {
                        pick -= weight;
                        if pick < 0 {
                            map.insert(alias.clone(), target.clone());
                            break;
                        }
                    }
                }
            }
            PoolAlias::RandomGroup { groups } => {
                let total: i32 = groups.iter().map(|(_, w)| *w).sum();
                if total > 0 {
                    let mut pick = rng.next_i32_bounded(total);
                    for (bindings, weight) in groups {
                        pick -= weight;
                        if pick < 0 {
                            for (alias, target) in bindings {
                                map.insert(alias.clone(), target.clone());
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    map
}

fn sample_start_height(config: &JigsawConfig, rng: &mut impl Random) -> i32 {
    match &config.start_height {
        StartHeight::Constant(y) => *y,
        StartHeight::Uniform { min, max } => rng.next_i32_between(*min, *max),
    }
}

/// Java integer midpoint used by vanilla jigsaw placement: `(min + max) / 2`.
const fn java_center(min: i32, max: i32) -> i32 {
    min.wrapping_add(max) / 2
}

static SYNTHETIC_BOTTOM_JIGSAW: Identifier = Identifier::new_static("minecraft", "bottom");
static SYNTHETIC_EMPTY_POOL: Identifier = Identifier::new_static("minecraft", "empty");

type PoolTemplateCache<'a> = FxHashMap<Identifier, Vec<&'a PoolElement>>;
type JigsawRotationCache<'a> = FxHashMap<Identifier, [Option<Vec<TransformedJigsaw<'a>>>; 4]>;
const CANDIDATE_DEDUPE_THRESHOLD: usize = 16;
const JIGSAW_PRIORITY_CACHE_THRESHOLD: usize = 16;
const QUEUE_HEAP_THRESHOLD: usize = 512;
const FREE_SPACE_OCTREE_THRESHOLD: usize = 512;

struct AssemblyScratch<'a> {
    parsed_candidates: FxHashSet<*const PoolElement>,
    source_jigsaw_indices: Vec<usize>,
    candidate_jigsaw_indices: Vec<usize>,
    jigsaw_order_scratch: Vec<usize>,
    jigsaw_priority_scratch: Vec<i32>,
    pool_max_y_cache: FxHashMap<Identifier, i32>,
    jigsaw_rotation_cache: JigsawRotationCache<'a>,
    jigsaw_priority_cache: FxHashMap<Identifier, Vec<i32>>,
    queue_order: u64,
}

impl AssemblyScratch<'_> {
    fn new() -> Self {
        Self {
            parsed_candidates: FxHashSet::default(),
            source_jigsaw_indices: Vec::new(),
            candidate_jigsaw_indices: Vec::new(),
            jigsaw_order_scratch: Vec::new(),
            jigsaw_priority_scratch: Vec::new(),
            pool_max_y_cache: FxHashMap::default(),
            jigsaw_rotation_cache: JigsawRotationCache::default(),
            jigsaw_priority_cache: FxHashMap::default(),
            queue_order: 0,
        }
    }
}

/// BFS queue entry ordered by descending `placement_priority`, FIFO within ties.
#[derive(Eq, PartialEq)]
struct PieceQueueEntry {
    priority: i32,
    order: u64,
    piece_idx: usize,
    depth: i32,
    context_idx: usize,
}

impl Ord for PieceQueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.order.cmp(&self.order))
    }
}

impl PartialOrd for PieceQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

enum PieceQueue {
    Small(Vec<PieceQueueEntry>),
    Large(BinaryHeap<PieceQueueEntry>),
}

impl PieceQueue {
    const fn new() -> Self {
        Self::Small(Vec::new())
    }

    fn push(&mut self, entry: PieceQueueEntry) {
        match self {
            Self::Small(entries) if entries.len() < QUEUE_HEAP_THRESHOLD => {
                entries.push(entry);
            }
            Self::Small(entries) => {
                let mut heap = BinaryHeap::from(mem::take(entries));
                heap.push(entry);
                *self = Self::Large(heap);
            }
            Self::Large(heap) => {
                heap.push(entry);
            }
        }
    }

    fn pop(&mut self) -> Option<PieceQueueEntry> {
        match self {
            Self::Small(entries) => {
                let best_idx = entries
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.cmp(b))
                    .map(|(idx, _)| idx)?;
                Some(entries.swap_remove(best_idx))
            }
            Self::Large(heap) => heap.pop(),
        }
    }
}

fn cached_pool_max_y_size(
    pool_key: &Identifier,
    pools: &FxHashMap<Identifier, TemplatePoolData>,
    templates: &FxHashMap<Identifier, TemplateData>,
    cache: &mut FxHashMap<Identifier, i32>,
) -> i32 {
    if let Some(size) = cache.get(pool_key) {
        return *size;
    }
    let size = pools
        .get(pool_key)
        .map_or(0, |pool| pool_max_y_size(pool, templates));
    cache.insert(pool_key.clone(), size);
    size
}

const fn rotation_index(rotation: Rotation) -> usize {
    match rotation {
        Rotation::None => 0,
        Rotation::Clockwise90 => 1,
        Rotation::Clockwise180 => 2,
        Rotation::CounterClockwise90 => 3,
    }
}

/// Vanilla-matching shuffle (reverse Fisher-Yates).
fn vanilla_shuffle<T>(list: &mut [T], rng: &mut LegacyRandom) {
    for i in (1..list.len()).rev() {
        let j = rng.next_i32_bounded((i + 1) as i32) as usize;
        list.swap(i, j);
    }
}

fn descending_priorities_into(template: &TemplateData, unique: &mut Vec<i32>) {
    unique.clear();
    for jigsaw in &template.jigsaws {
        if !unique.contains(&jigsaw.selection_priority) {
            unique.push(jigsaw.selection_priority);
        }
    }
    if unique.len() > 1 {
        unique.sort_unstable_by_key(|priority| Reverse(*priority));
    }
}

fn descending_priorities(template: &TemplateData) -> Vec<i32> {
    let mut unique = Vec::new();
    descending_priorities_into(template, &mut unique);
    unique
}

fn cached_descending_priorities<'cache>(
    location: &Identifier,
    template: &TemplateData,
    cache: &'cache mut FxHashMap<Identifier, Vec<i32>>,
) -> &'cache [i32] {
    cache
        .entry(location.clone())
        .or_insert_with(|| descending_priorities(template))
}

fn cached_runtime_rotated_jigsaws<'cache, 'a>(
    location: &Identifier,
    template: &'a TemplateData,
    rotation: Rotation,
    cache: &'cache mut JigsawRotationCache<'a>,
) -> &'cache [TransformedJigsaw<'a>] {
    let idx = rotation_index(rotation);
    let by_rotation = cache
        .entry(location.clone())
        .or_insert_with(|| array::from_fn(|_| None));
    by_rotation[idx].get_or_insert_with(|| transform_template_jigsaws(template, rotation))
}

fn transform_template_jigsaws(
    template: &TemplateData,
    rotation: Rotation,
) -> Vec<TransformedJigsaw<'_>> {
    template
        .jigsaws
        .iter()
        .map(|jigsaw| {
            let pos = rotation.transform_pos(IVec3::from(jigsaw.pos), IVec3::ZERO);
            TransformedJigsaw {
                pos,
                orientation: jigsaw.orientation.rotate(rotation),
                name: &jigsaw.name,
                target: &jigsaw.target,
                pool: &jigsaw.pool,
                joint: jigsaw.joint,
                placement_priority: jigsaw.placement_priority,
            }
        })
        .collect()
}

fn shuffle_jigsaw_indices_into(
    template: &TemplateData,
    priorities: &[i32],
    rng: &mut LegacyRandom,
    out: &mut Vec<usize>,
    order_scratch: &mut Vec<usize>,
) {
    out.clear();
    if template.jigsaws.is_empty() {
        return;
    }
    out.extend(0..template.jigsaws.len());
    vanilla_shuffle(out, rng);
    order_jigsaw_indices_by_priorities(template, priorities, out, order_scratch);
}

fn order_jigsaw_indices_by_priorities(
    template: &TemplateData,
    priorities: &[i32],
    out: &mut Vec<usize>,
    scratch: &mut Vec<usize>,
) {
    if priorities.len() <= 1 {
        return;
    }
    scratch.clear();
    scratch.extend_from_slice(out);
    out.clear();
    for &priority in priorities {
        out.extend(
            scratch
                .iter()
                .copied()
                .filter(|&idx| template.jigsaws[idx].selection_priority == priority),
        );
    }
}

fn shuffle_jigsaw_indices_with_priority_cache(
    location: &Identifier,
    template: &TemplateData,
    rng: &mut LegacyRandom,
    out: &mut Vec<usize>,
    order_scratch: &mut Vec<usize>,
    priority_scratch: &mut Vec<i32>,
    priority_cache: &mut FxHashMap<Identifier, Vec<i32>>,
) {
    if template.jigsaws.len() > JIGSAW_PRIORITY_CACHE_THRESHOLD {
        let priorities = cached_descending_priorities(location, template, priority_cache);
        shuffle_jigsaw_indices_into(template, priorities, rng, out, order_scratch);
    } else {
        descending_priorities_into(template, priority_scratch);
        shuffle_jigsaw_indices_into(template, priority_scratch, rng, out, order_scratch);
    }
}

/// Consumes the same RNG draws as a failed placement attempt for a duplicate pool element.
///
/// Vanilla keeps weighted duplicates, but each attempt exhausts every rotation
/// and target jigsaw before moving on. With unchanged free space, a later
/// identical duplicate cannot succeed if the first one failed; only the RNG
/// draws need to be preserved.
fn prime_duplicate_candidate_rng(
    element: &PoolElement,
    templates: &FxHashMap<Identifier, TemplateData>,
    rotations: [Rotation; 4],
    rng: &mut LegacyRandom,
    scratch: &mut AssemblyScratch<'_>,
) {
    for _rotation in rotations {
        if let Some(location) = element_location(element)
            && let Some(template) = templates.get(location)
        {
            shuffle_jigsaw_indices_with_priority_cache(
                location,
                template,
                rng,
                &mut scratch.candidate_jigsaw_indices,
                &mut scratch.jigsaw_order_scratch,
                &mut scratch.jigsaw_priority_scratch,
                &mut scratch.jigsaw_priority_cache,
            );
        }
        // Feature/Empty elements do not shuffle jigsaws; no RNG to prime here.
    }
}

fn feature_synthetic_jigsaw() -> TransformedJigsaw<'static> {
    TransformedJigsaw {
        pos: IVec3::ZERO,
        orientation: JigsawOrientation::DownSouth,
        name: &SYNTHETIC_BOTTOM_JIGSAW,
        target: &SYNTHETIC_EMPTY_POOL,
        pool: &SYNTHETIC_EMPTY_POOL,
        joint: JointType::Rollable,
        placement_priority: 0,
    }
}

fn shuffled_element_jigsaws<'a>(
    element: &PoolElement,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<TransformedJigsaw<'a>> {
    match element {
        PoolElement::Single { location, .. } | PoolElement::LegacySingle { location, .. } => {
            let Some(template) = templates.get(location) else {
                return Vec::new();
            };

            let rotated = transform_template_jigsaws(template, rotation);
            let mut priorities = Vec::new();
            descending_priorities_into(template, &mut priorities);
            let mut shuffle_indices = Vec::new();
            let mut order_scratch = Vec::new();
            shuffle_jigsaw_indices_into(
                template,
                &priorities,
                rng,
                &mut shuffle_indices,
                &mut order_scratch,
            );
            shuffle_indices
                .into_iter()
                .map(|idx| rotated[idx])
                .collect()
        }
        PoolElement::Feature { .. } => vec![feature_synthetic_jigsaw()],
        PoolElement::List { elements, .. } => elements.first().map_or_else(Vec::new, |element| {
            shuffled_element_jigsaws(element, templates, rotation, rng)
        }),
        PoolElement::Empty => Vec::new(),
    }
}

/// Active source connector during jigsaw BFS.
struct ActiveSourceJigsaw<'a> {
    block: TransformedJigsaw<'a>,
    pos: IVec3,
}

impl ActiveSourceJigsaw<'_> {
    fn can_attach_to(&self, target: &TransformedJigsaw<'_>) -> bool {
        if self.block.orientation.front_direction()
            != target.orientation.front_direction().opposite()
        {
            return false;
        }
        if self.block.joint == JointType::Aligned
            && self.block.orientation.top_direction() != target.orientation.top_direction()
        {
            return false;
        }
        self.block.target == target.name
    }
}

/// A jigsaw block with its position transformed by rotation.
#[derive(Clone, Copy)]
struct TransformedJigsaw<'a> {
    pos: IVec3,
    orientation: JigsawOrientation,
    name: &'a Identifier,
    target: &'a Identifier,
    pool: &'a Identifier,
    joint: JointType,
    placement_priority: i32,
}

/// Gets the template location from a pool element.
///
/// For `List` elements, delegates to the first sub-element matching vanilla's
/// `ListPoolElement` which uses `elements.get(0)` for jigsaws and BB.
fn element_location(element: &PoolElement) -> Option<&Identifier> {
    match element {
        PoolElement::Single { location, .. } | PoolElement::LegacySingle { location, .. } => {
            Some(location)
        }
        PoolElement::List { elements, .. } => elements.first().and_then(element_location),
        _ => None,
    }
}

/// Vanilla's `StructureTemplatePool.getMaxSize` — max Y span across all templates.
fn pool_max_y_size(
    pool: &TemplatePoolData,
    templates: &FxHashMap<Identifier, TemplateData>,
) -> i32 {
    pool.elements
        .iter()
        .filter_map(|(element, _)| {
            let (PoolElement::Single { location: loc, .. }
            | PoolElement::LegacySingle { location: loc, .. }) = element
            else {
                return None;
            };
            templates.get(loc).map(|t| t.size[1])
        })
        .max()
        .unwrap_or(0)
}

/// Gets the bounding box for a pool element at a position with rotation.
///
/// Feature elements return a 1×1×1 BB at the given position, matching
/// vanilla's `FeaturePoolElement.getBoundingBox`.
/// List elements return the encapsulating BB of all sub-elements, matching
/// vanilla's `ListPoolElement.getBoundingBox`.
fn element_bounding_box(
    element: &PoolElement,
    templates: &FxHashMap<Identifier, TemplateData>,
    pos: IVec3,
    rotation: Rotation,
) -> Option<BoundingBox> {
    match element {
        PoolElement::Feature { .. } => Some(BoundingBox::new(pos, pos)),
        PoolElement::List { elements, .. } => {
            let mut result: Option<BoundingBox> = None;
            for sub in elements {
                if let Some(sub_bb) = element_bounding_box(sub, templates, pos, rotation) {
                    result = Some(match result {
                        Some(prev) => BoundingBox::new(
                            IVec3::new(
                                prev.min_x().min(sub_bb.min_x()),
                                prev.min_y().min(sub_bb.min_y()),
                                prev.min_z().min(sub_bb.min_z()),
                            ),
                            IVec3::new(
                                prev.max_x().max(sub_bb.max_x()),
                                prev.max_y().max(sub_bb.max_y()),
                                prev.max_z().max(sub_bb.max_z()),
                            ),
                        ),
                        None => sub_bb,
                    });
                }
            }
            result
        }
        _ => {
            let location = element_location(element)?;
            let template = templates.get(location)?;
            let size = IVec3::from(template.size);
            Some(rotation.get_bounding_box(pos, size))
        }
    }
}

fn candidate_bounding_box_at_origin(
    element: &PoolElement,
    templates: &FxHashMap<Identifier, TemplateData>,
    template: Option<&TemplateData>,
    rotation: Rotation,
) -> Option<BoundingBox> {
    match element {
        PoolElement::Single { .. } | PoolElement::LegacySingle { .. } => {
            let size = IVec3::from(template?.size);
            Some(rotation.get_bounding_box(IVec3::ZERO, size))
        }
        _ => element_bounding_box(element, templates, IVec3::ZERO, rotation),
    }
}

fn expand_pool_weights(pool: &TemplatePoolData) -> Vec<&PoolElement> {
    let mut expanded = Vec::with_capacity(pool.elements.iter().map(|(_, w)| *w as usize).sum());
    for (element, weight) in &pool.elements {
        for _ in 0..*weight {
            expanded.push(element);
        }
    }
    expanded
}

/// Appends vanilla's `StructureTemplatePool.getShuffledTemplates` to `out`.
fn append_shuffled_templates_cached<'a>(
    pool: &'a TemplatePoolData,
    cache: &mut PoolTemplateCache<'a>,
    rng: &mut LegacyRandom,
    out: &mut Vec<&'a PoolElement>,
) {
    let expanded = cache
        .entry(pool.key.clone())
        .or_insert_with(|| expand_pool_weights(pool));
    let start = out.len();
    out.extend(expanded.iter().copied());
    vanilla_shuffle(&mut out[start..], rng);
}

/// Vanilla's `StructureTemplatePool.getRandomTemplate`.
fn get_random_template<'a>(pool: &'a TemplatePoolData, rng: &mut LegacyRandom) -> &'a PoolElement {
    let expanded = expand_pool_weights(pool);
    if expanded.is_empty() {
        static EMPTY: PoolElement = PoolElement::Empty;
        return &EMPTY;
    }
    let idx = rng.next_i32_bounded(expanded.len() as i32) as usize;
    expanded[idx]
}

/// Free-space tracker. Small contexts use a flat list; larger contexts switch
/// to `BoxOctree` for nearby-box queries.
enum FreeSpace {
    Small {
        boundary: BoundingBox,
        occupied: Vec<BoundingBox>,
    },
    Large {
        occupied: BoxOctree,
    },
}

impl FreeSpace {
    const fn new(constraint: BoundingBox) -> Self {
        Self::Small {
            boundary: constraint,
            occupied: Vec::new(),
        }
    }

    fn add_box(&mut self, bbox: BoundingBox) {
        match self {
            Self::Small { occupied, .. } if occupied.len() < FREE_SPACE_OCTREE_THRESHOLD => {
                occupied.push(bbox);
            }
            Self::Small {
                boundary, occupied, ..
            } => {
                let mut octree = BoxOctree::new(*boundary);
                for stored in occupied.drain(..) {
                    octree.add_box(stored);
                }
                octree.add_box(bbox);
                *self = Self::Large { occupied: octree };
            }
            Self::Large { occupied } => {
                occupied.add_box(bbox);
            }
        }
    }

    fn collides(&self, candidate: &BoundingBox) -> bool {
        match self {
            Self::Small {
                boundary, occupied, ..
            } => {
                if candidate.min_x() < boundary.min_x()
                    || candidate.max_x() > boundary.max_x()
                    || candidate.min_y() < boundary.min_y()
                    || candidate.max_y() > boundary.max_y()
                    || candidate.min_z() < boundary.min_z()
                    || candidate.max_z() > boundary.max_z()
                {
                    return true;
                }

                // For integer piece boxes, vanilla's deflated AABB collision is
                // equivalent to inclusive `BoundingBox` intersection.
                occupied.iter().any(|stored| candidate.intersects(*stored))
            }
            Self::Large { occupied } => {
                !occupied.within_bounds_but_not_intersecting_children(*candidate)
            }
        }
    }
}

/// Result of a successful jigsaw assembly.
pub struct AssemblyResult {
    /// The placed pieces.
    pub pieces: Vec<PlacedPiece>,
    /// The biome check position (centerX, centerY, centerZ from the `GenerationStub`).
    pub biome_check_pos: IVec3,
}

struct StartedAssembly {
    pieces: Vec<PlacedPiece>,
    biome_check_pos: IVec3,
}

/// Vanilla's `JigsawPlacement.addPieces` before the lazy `GenerationStub` child builder.
#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla's addPieces call surface"
)]
fn start_assembly(
    config: &JigsawConfig,
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    pools: &FxHashMap<Identifier, TemplatePoolData>,
    templates: &FxHashMap<Identifier, TemplateData>,
    alias_map: &FxHashMap<Identifier, Identifier>,
    get_height: &mut dyn FnMut(i32, i32) -> i32,
    min_y: i32,
    max_y: i32,
) -> Option<StartedAssembly> {
    let start_y = sample_start_height(config, rng);
    let start_x = chunk_x * 16;
    let start_z = chunk_z * 16;
    let center_rotation = Rotation::get_random(rng);

    let start_pool_key = alias_map
        .get(&config.start_pool)
        .unwrap_or(&config.start_pool);
    let start_pool = pools.get(start_pool_key)?;
    let center_element = get_random_template(start_pool, rng);
    if center_element.is_empty() {
        return None;
    }

    let anchor_offset = if let Some(ref jigsaw_name) = config.start_jigsaw_name {
        shuffled_element_jigsaws(center_element, templates, center_rotation, rng)
            .into_iter()
            .find_map(|block| (block.name == jigsaw_name).then_some(block.pos))?
    } else {
        IVec3::ZERO
    };

    let adjusted = IVec3::new(
        start_x - anchor_offset.x,
        start_y - anchor_offset.y,
        start_z - anchor_offset.z,
    );

    let center_bb = element_bounding_box(center_element, templates, adjusted, center_rotation)?;

    let bottom_y = if config.project_start_to_heightmap.is_some() {
        let mid_x = java_center(center_bb.min_x(), center_bb.max_x());
        let mid_z = java_center(center_bb.min_z(), center_bb.max_z());
        start_y + get_height(mid_x, mid_z)
    } else {
        adjusted.y
    };

    let ground_level_delta = center_element.projection().ground_level_delta();
    let dy = bottom_y - (center_bb.min_y() + ground_level_delta);
    let center_bb = BoundingBox::new(
        IVec3::new(center_bb.min_x(), center_bb.min_y() + dy, center_bb.min_z()),
        IVec3::new(center_bb.max_x(), center_bb.max_y() + dy, center_bb.max_z()),
    );
    let adjusted_y = adjusted.y + dy;

    let padding = &config.dimension_padding;
    if center_bb.min_y() < min_y + padding.bottom || center_bb.max_y() > max_y - 1 - padding.top {
        return None;
    }

    let pieces = vec![PlacedPiece {
        element: center_element.clone(),
        template_location: element_location(center_element).cloned(),
        position: IVec3::new(adjusted.x, adjusted_y, adjusted.z),
        rotation: center_rotation,
        bounding_box: center_bb,
        assembly_bb: center_bb,
        ground_level_delta,
        projection: center_element.projection(),
        depth: 0,
        junctions: Vec::new(),
    }];

    let center_stub_x = java_center(center_bb.min_x(), center_bb.max_x());
    let center_stub_z = java_center(center_bb.min_z(), center_bb.max_z());
    let center_stub_y = bottom_y + anchor_offset.y;
    let biome_check_pos = IVec3::new(center_stub_x, center_stub_y, center_stub_z);

    Some(StartedAssembly {
        pieces,
        biome_check_pos,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla's addPieces child-builder call surface"
)]
fn finish_assembly<'a>(
    mut started: StartedAssembly,
    config: &JigsawConfig,
    rng: &mut LegacyRandom,
    pools: &'a FxHashMap<Identifier, TemplatePoolData>,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    alias_map: &FxHashMap<Identifier, Identifier>,
    get_height: &mut dyn FnMut(i32, i32) -> i32,
    min_y: i32,
    max_y: i32,
) -> AssemblyResult {
    let biome_check_pos = started.biome_check_pos;

    if config.max_depth <= 0 {
        return AssemblyResult {
            pieces: started.pieces,
            biome_check_pos,
        };
    }

    let Some(center_piece) = started.pieces.first() else {
        return AssemblyResult {
            pieces: started.pieces,
            biome_check_pos,
        };
    };
    let center_bb = center_piece.assembly_bb;
    let center_stub_x = biome_check_pos.x;
    let center_stub_y = biome_check_pos.y;
    let center_stub_z = biome_check_pos.z;

    let max_dist = config.max_distance_from_center;
    let constraint_bb = BoundingBox::new(
        IVec3::new(
            center_stub_x - max_dist,
            (center_stub_y - max_dist).max(min_y + config.dimension_padding.bottom),
            center_stub_z - max_dist,
        ),
        IVec3::new(
            center_stub_x + max_dist,
            (center_stub_y + max_dist).min(max_y - 1 - config.dimension_padding.top),
            center_stub_z + max_dist,
        ),
    );

    let mut free_spaces: Vec<FreeSpace> = {
        let mut space = FreeSpace::new(constraint_bb);
        space.add_box(center_bb);
        vec![space]
    };
    let mut pool_template_cache = PoolTemplateCache::default();
    let mut assembly_scratch = AssemblyScratch::new();
    let mut queue = PieceQueue::new();

    try_placing_children(
        0,
        0,
        0,
        config,
        pools,
        templates,
        alias_map,
        &mut pool_template_cache,
        &mut assembly_scratch,
        &mut started.pieces,
        &mut free_spaces,
        &mut queue,
        rng,
        get_height,
    );

    while let Some(entry) = queue.pop() {
        try_placing_children(
            entry.piece_idx,
            entry.depth,
            entry.context_idx,
            config,
            pools,
            templates,
            alias_map,
            &mut pool_template_cache,
            &mut assembly_scratch,
            &mut started.pieces,
            &mut free_spaces,
            &mut queue,
            rng,
            get_height,
        );
    }

    AssemblyResult {
        pieces: started.pieces,
        biome_check_pos,
    }
}

/// Vanilla's `JigsawPlacement.addPieces`. Returns `None` on failure (empty start
/// pool, dimension padding violation, etc.).
#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla's addPieces call surface"
)]
#[expect(
    clippy::implicit_hasher,
    reason = "FxHashMap avoids SipHash overhead on Identifier lookups"
)]
pub fn assemble(
    config: &JigsawConfig,
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    pools: &FxHashMap<Identifier, TemplatePoolData>,
    templates: &FxHashMap<Identifier, TemplateData>,
    alias_map: &FxHashMap<Identifier, Identifier>,
    get_height: &mut dyn FnMut(i32, i32) -> i32,
    min_y: i32,
    max_y: i32,
) -> Option<AssemblyResult> {
    let started = start_assembly(
        config, rng, chunk_x, chunk_z, pools, templates, alias_map, get_height, min_y, max_y,
    )?;
    Some(finish_assembly(
        started, config, rng, pools, templates, alias_map, get_height, min_y, max_y,
    ))
}

/// Registered under `minecraft:jigsaw` for pool-based structures such as villages,
/// bastions, ancient cities, and trail ruins.
pub struct JigsawStructure;

impl Structure for JigsawStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        _rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let config = structure.config.as_jigsaw()?;

        let mut alias_position_rng = LegacyRandom::from_seed(0);
        alias_position_rng.set_large_feature_seed(ctx.seed(), ctx.chunk_x(), ctx.chunk_z());
        let start_y = sample_start_height(config, &mut alias_position_rng);
        let mut alias_source = LegacyRandom::from_seed(ctx.seed() as u64);
        let mut alias_rng =
            alias_source
                .next_positional()
                .at(ctx.chunk_min_x(), start_y, ctx.chunk_min_z());
        let alias_map = resolve_aliases(&config.pool_aliases, &mut alias_rng);

        let mut assembly_rng = LegacyRandom::from_seed(0);
        assembly_rng.set_large_feature_seed(ctx.seed(), ctx.chunk_x(), ctx.chunk_z());

        let started = {
            let mut get_height = |x: i32, z: i32| ctx.terrain_surface_height(x, z, false);
            start_assembly(
                config,
                &mut assembly_rng,
                ctx.chunk_x(),
                ctx.chunk_z(),
                ctx.template_pools(),
                ctx.templates(),
                &alias_map,
                &mut get_height,
                ctx.min_y(),
                ctx.max_y(),
            )?
        };

        if started.pieces.is_empty() {
            return None;
        }

        let biome = ctx.biome_at(
            started.biome_check_pos.x,
            started.biome_check_pos.y,
            started.biome_check_pos.z,
        );
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let assembly = {
            let mut get_height = |x: i32, z: i32| ctx.terrain_surface_height(x, z, false);
            finish_assembly(
                started,
                config,
                &mut assembly_rng,
                ctx.template_pools(),
                ctx.templates(),
                &alias_map,
                &mut get_height,
                ctx.min_y(),
                ctx.max_y(),
            )
        };

        let pieces = assembly
            .pieces
            .into_iter()
            .map(|piece| StructurePiece {
                piece_type: Identifier::new_static("minecraft", "jigsaw"),
                bounding_box: piece.assembly_bb,
                gen_depth: 0,
                orientation: None,
                payload: StructurePiecePayload::Jigsaw(JigsawPieceData {
                    pool_element: piece.element,
                    position: piece.position,
                    rotation: piece.rotation,
                    liquid_settings: config.liquid_settings,
                }),
                ground_level_delta: piece.ground_level_delta,
                junctions: piece.junctions,
                projection: Some(piece.projection),
            })
            .collect();

        Some(GenerationStub {
            position: (
                assembly.biome_check_pos.x,
                assembly.biome_check_pos.y,
                assembly.biome_check_pos.z,
            ),
            pieces,
        })
    }
}

/// Vanilla's `tryPlacingChildren`. `context_idx` is this piece's collision context
/// in `free_spaces` — external children get the parent's context, internal
/// children get the parent's internal free space.
#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla's tryPlacingChildren signature"
)]
#[expect(
    clippy::too_many_lines,
    reason = "inlined to mirror vanilla's source-jigsaw/child-pool loop"
)]
fn try_placing_children<'a>(
    source_idx: usize,
    depth: i32,
    context_idx: usize,
    config: &JigsawConfig,
    pools: &'a FxHashMap<Identifier, TemplatePoolData>,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    alias_map: &FxHashMap<Identifier, Identifier>,
    pool_template_cache: &mut PoolTemplateCache<'a>,
    scratch: &mut AssemblyScratch<'a>,
    pieces: &mut Vec<PlacedPiece>,
    free_spaces: &mut Vec<FreeSpace>,
    queue: &mut PieceQueue,
    rng: &mut LegacyRandom,
    get_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    let source_piece = &pieces[source_idx];
    let source_location = element_location(&source_piece.element).cloned();
    let source_element_empty = source_piece.element.is_empty();
    let source_rotation = source_piece.rotation;
    let origin = source_piece.position;
    let source_bb = source_piece.assembly_bb;
    let source_projection = source_piece.projection;
    let source_ground_level_delta = source_piece.ground_level_delta;
    let source_template = source_location
        .as_ref()
        .and_then(|location| templates.get(location).map(|template| (location, template)));

    if let Some((location, template)) = source_template {
        shuffle_jigsaw_indices_with_priority_cache(
            location,
            template,
            rng,
            &mut scratch.source_jigsaw_indices,
            &mut scratch.jigsaw_order_scratch,
            &mut scratch.jigsaw_priority_scratch,
            &mut scratch.jigsaw_priority_cache,
        );
        if scratch.source_jigsaw_indices.is_empty() {
            return;
        }
    } else if source_element_empty {
        return;
    }

    let source_jigsaw_count = if source_template.is_some() {
        scratch.source_jigsaw_indices.len()
    } else {
        1
    };
    let source_box_y = source_bb.min_y();
    let source_rigid = source_projection == Projection::Rigid;

    let mut internal_ctx_idx: Option<usize> = None;
    let mut candidates: Vec<&PoolElement> = Vec::new();

    'source_jigsaw: for source_jigsaw_i in 0..source_jigsaw_count {
        let source = if let Some((location, template)) = source_template {
            let rotated = cached_runtime_rotated_jigsaws(
                location,
                template,
                source_rotation,
                &mut scratch.jigsaw_rotation_cache,
            );
            let block = rotated[scratch.source_jigsaw_indices[source_jigsaw_i]];
            let pos = block.pos + origin;
            ActiveSourceJigsaw { block, pos }
        } else {
            ActiveSourceJigsaw {
                block: feature_synthetic_jigsaw(),
                pos: origin,
            }
        };
        candidates.clear();
        let front = source.block.orientation.front_direction();
        let foff = front.offset_vec();
        let target_jigsaw_world = source.pos + foff;

        let source_jigsaw_local_y = source.pos.y - source_box_y;

        let pool_key = alias_map
            .get(source.block.pool)
            .unwrap_or(source.block.pool);
        let raw_pool = pools.get(pool_key);
        let target_pool = raw_pool.filter(|p| !p.elements.is_empty());
        let fallback_pool = raw_pool
            .and_then(|p| pools.get(&p.fallback))
            .filter(|p| !p.elements.is_empty());

        let attach_inside = source_bb.contains_xyz(
            target_jigsaw_world.x,
            target_jigsaw_world.y,
            target_jigsaw_world.z,
        );

        if depth != config.max_depth
            && let Some(pool) = target_pool
        {
            append_shuffled_templates_cached(pool, pool_template_cache, rng, &mut candidates);
        }
        if let Some(fallback) = fallback_pool {
            append_shuffled_templates_cached(fallback, pool_template_cache, rng, &mut candidates);
        }

        let placement_priority = source.block.placement_priority;
        let mut source_jigsaw_base_height: Option<i32> = None;
        let dedupe_candidates = candidates.len() > CANDIDATE_DEDUPE_THRESHOLD;
        if dedupe_candidates {
            scratch.parsed_candidates.clear();
        }

        for &candidate_element in &candidates {
            if candidate_element.is_empty() {
                break;
            }

            let rotations = Rotation::get_shuffled(rng);
            if dedupe_candidates
                && !scratch
                    .parsed_candidates
                    .insert(ptr::from_ref(candidate_element))
            {
                prime_duplicate_candidate_rng(
                    candidate_element,
                    templates,
                    rotations,
                    rng,
                    scratch,
                );
                continue;
            }

            let candidate_location = element_location(candidate_element);
            let candidate_template = candidate_location
                .and_then(|location| templates.get(location).map(|template| (location, template)));
            let candidate_template_data = candidate_template.map(|(_, template)| template);
            let candidate_projection = candidate_element.projection();
            let candidate_rigid = candidate_projection == Projection::Rigid;

            for candidate_rotation in rotations {
                let expand_to = if config.use_expansion_hack {
                    if let Some((hack_location, template_data)) = candidate_template {
                        let hack_box = candidate_rotation
                            .get_bounding_box(IVec3::ZERO, IVec3::from(template_data.size));
                        if hack_box.max_y() - hack_box.min_y() < 16 {
                            let rotated = cached_runtime_rotated_jigsaws(
                                hack_location,
                                template_data,
                                candidate_rotation,
                                &mut scratch.jigsaw_rotation_cache,
                            );
                            rotated
                                .iter()
                                .map(|j| {
                                    let pos = j.pos;
                                    let front = j.orientation.front_direction();
                                    let front_pos = pos + front.offset_vec();
                                    if !hack_box.contains_xyz(front_pos.x, front_pos.y, front_pos.z)
                                    {
                                        return 0;
                                    }
                                    let child_pool_key = alias_map.get(j.pool).unwrap_or(j.pool);
                                    let child_pool_size = cached_pool_max_y_size(
                                        child_pool_key,
                                        pools,
                                        templates,
                                        &mut scratch.pool_max_y_cache,
                                    );
                                    let child_fallback_size =
                                        pools.get(child_pool_key).map_or(0, |pool| {
                                            cached_pool_max_y_size(
                                                &pool.fallback,
                                                pools,
                                                templates,
                                                &mut scratch.pool_max_y_cache,
                                            )
                                        });
                                    child_pool_size.max(child_fallback_size)
                                })
                                .max()
                                .unwrap_or(0)
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };

                let mut candidate_bb_at_origin: Option<BoundingBox> = None;

                let mut try_target_jigsaw = |target: &TransformedJigsaw<'_>| -> bool {
                    if !source.can_attach_to(target) {
                        return false;
                    }

                    let target_jigsaw_local = target.pos;

                    let raw_target = IVec3::new(
                        target_jigsaw_world.x - target_jigsaw_local.x,
                        0,
                        target_jigsaw_world.z - target_jigsaw_local.z,
                    );

                    let raw_bb = if let Some(bb) = candidate_bb_at_origin {
                        bb.translate(IVec3::new(raw_target.x, 0, raw_target.z))
                    } else {
                        let Some(bb) = candidate_bounding_box_at_origin(
                            candidate_element,
                            templates,
                            candidate_template_data,
                            candidate_rotation,
                        ) else {
                            return false;
                        };
                        candidate_bb_at_origin = Some(bb);
                        bb.translate(IVec3::new(raw_target.x, 0, raw_target.z))
                    };

                    let target_jigsaw_local_y = target_jigsaw_local.y;
                    let delta_y = source_jigsaw_local_y - target_jigsaw_local_y + foff.y;

                    let target_box_y = if source_rigid && candidate_rigid {
                        source_box_y + delta_y
                    } else {
                        let base_height = *source_jigsaw_base_height
                            .get_or_insert_with(|| get_height(source.pos.x, source.pos.z));
                        base_height - target_jigsaw_local_y
                    };

                    let y_offset = target_box_y - raw_bb.min_y();
                    let candidate_bb = BoundingBox::new(
                        IVec3::new(raw_bb.min_x(), raw_bb.min_y() + y_offset, raw_bb.min_z()),
                        IVec3::new(raw_bb.max_x(), raw_bb.max_y() + y_offset, raw_bb.max_z()),
                    );
                    let target_position =
                        IVec3::new(raw_target.x, raw_bb.min_y() + y_offset, raw_target.z);

                    let expanded_bb = if expand_to > 0 {
                        let new_size =
                            (expand_to + 1).max(candidate_bb.max_y() - candidate_bb.min_y());
                        BoundingBox::new(
                            IVec3::new(
                                candidate_bb.min_x(),
                                candidate_bb.min_y(),
                                candidate_bb.min_z(),
                            ),
                            IVec3::new(
                                candidate_bb.max_x(),
                                candidate_bb.min_y() + new_size,
                                candidate_bb.max_z(),
                            ),
                        )
                    } else {
                        candidate_bb
                    };

                    let effective_ctx = if attach_inside {
                        *internal_ctx_idx.get_or_insert_with(|| {
                            free_spaces.push(FreeSpace::new(source_bb));
                            free_spaces.len() - 1
                        })
                    } else {
                        context_idx
                    };

                    if free_spaces[effective_ctx].collides(&expanded_bb) {
                        return false;
                    }

                    free_spaces[effective_ctx].add_box(expanded_bb);

                    let target_ground_level_delta = if candidate_rigid {
                        source_ground_level_delta - delta_y
                    } else {
                        candidate_projection.ground_level_delta()
                    };

                    let junction_y = if source_rigid {
                        source_box_y + source_jigsaw_local_y
                    } else if candidate_rigid {
                        target_box_y + target_jigsaw_local_y
                    } else {
                        let base_height = *source_jigsaw_base_height
                            .get_or_insert_with(|| get_height(source.pos.x, source.pos.z));
                        base_height + delta_y / 2
                    };

                    pieces[source_idx].junctions.push(JigsawJunction {
                        source_pos: IVec3::new(
                            target_jigsaw_world.x,
                            junction_y - source_jigsaw_local_y + source_ground_level_delta,
                            target_jigsaw_world.z,
                        ),
                        delta_y,
                        dest_projection: candidate_projection,
                    });

                    let new_piece_idx = pieces.len();
                    let mut target_piece = PlacedPiece {
                        element: candidate_element.clone(),
                        template_location: candidate_location.cloned(),
                        position: target_position,
                        rotation: candidate_rotation,
                        bounding_box: candidate_bb,
                        assembly_bb: expanded_bb,
                        ground_level_delta: target_ground_level_delta,
                        projection: candidate_projection,
                        depth: depth + 1,
                        junctions: Vec::new(),
                    };

                    target_piece.junctions.push(JigsawJunction {
                        source_pos: IVec3::new(
                            source.pos.x,
                            junction_y - target_jigsaw_local_y + target_ground_level_delta,
                            source.pos.z,
                        ),
                        delta_y: -delta_y,
                        dest_projection: source_projection,
                    });

                    pieces.push(target_piece);

                    if depth < config.max_depth {
                        scratch.queue_order += 1;
                        queue.push(PieceQueueEntry {
                            priority: placement_priority,
                            order: scratch.queue_order,
                            piece_idx: new_piece_idx,
                            depth: depth + 1,
                            context_idx: effective_ctx,
                        });
                    }

                    true
                };

                if let Some((location, template)) = candidate_template {
                    let rotated = cached_runtime_rotated_jigsaws(
                        location,
                        template,
                        candidate_rotation,
                        &mut scratch.jigsaw_rotation_cache,
                    );
                    shuffle_jigsaw_indices_with_priority_cache(
                        location,
                        template,
                        rng,
                        &mut scratch.candidate_jigsaw_indices,
                        &mut scratch.jigsaw_order_scratch,
                        &mut scratch.jigsaw_priority_scratch,
                        &mut scratch.jigsaw_priority_cache,
                    );
                    for &target_jigsaw_idx in &scratch.candidate_jigsaw_indices {
                        if try_target_jigsaw(&rotated[target_jigsaw_idx]) {
                            continue 'source_jigsaw;
                        }
                    }
                } else if try_target_jigsaw(&feature_synthetic_jigsaw()) {
                    continue 'source_jigsaw;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::structure::DimensionPadding;

    fn bbox(min: IVec3, max: IVec3) -> BoundingBox {
        BoundingBox::new(min, max)
    }

    fn free_space_boxes() -> Vec<BoundingBox> {
        let mut boxes = Vec::with_capacity(FREE_SPACE_OCTREE_THRESHOLD + 2);
        boxes.push(bbox(IVec3::new(-1, -1, -1), IVec3::new(1, 1, 1)));

        for y in [-50, 50] {
            for x in 0..16 {
                for z in 0..16 {
                    let min = IVec3::new(-120 + x * 16, y, -120 + z * 16);
                    boxes.push(bbox(min, min + IVec3::ONE));
                }
            }
        }

        boxes.push(bbox(IVec3::new(-1, -1, -1), IVec3::new(1, 1, 1)));
        boxes
    }

    #[test]
    fn free_space_large_matches_small_scan_after_octree_transition() {
        let boundary = bbox(IVec3::new(-128, -64, -128), IVec3::new(128, 64, 128));
        let boxes = free_space_boxes();
        let small = FreeSpace::Small {
            boundary,
            occupied: boxes.clone(),
        };
        let mut large = FreeSpace::new(boundary);
        for bbox in boxes {
            large.add_box(bbox);
        }

        assert!(matches!(large, FreeSpace::Large { .. }));

        let candidates = [
            bbox(IVec3::new(1, 1, 1), IVec3::new(3, 3, 3)),
            bbox(IVec3::new(2, 2, 2), IVec3::new(4, 4, 4)),
            bbox(IVec3::new(-2, 10, -2), IVec3::new(2, 12, 2)),
            bbox(IVec3::new(124, 0, 0), IVec3::new(128, 2, 2)),
            bbox(IVec3::new(127, 0, 0), IVec3::new(129, 2, 2)),
        ];

        for candidate in candidates {
            assert_eq!(
                large.collides(&candidate),
                small.collides(&candidate),
                "collision mismatch for {candidate:?}"
            );
        }
    }

    #[test]
    fn start_jigsaw_name_can_anchor_feature_pool_element() {
        let pool_key = Identifier::vanilla_static("test/feature_start");
        let mut pools = FxHashMap::default();
        pools.insert(
            pool_key.clone(),
            TemplatePoolData {
                key: pool_key.clone(),
                fallback: Identifier::vanilla_static("empty"),
                elements: vec![(
                    PoolElement::Feature {
                        feature: Identifier::vanilla_static("oak"),
                        projection: Projection::Rigid,
                    },
                    1,
                )],
            },
        );
        let templates = FxHashMap::default();
        let alias_map = FxHashMap::default();
        let config = JigsawConfig {
            start_pool: pool_key,
            max_depth: 0,
            use_expansion_hack: false,
            project_start_to_heightmap: None,
            start_height: StartHeight::Constant(70),
            max_distance_from_center: 80,
            start_jigsaw_name: Some(Identifier::vanilla_static("bottom")),
            dimension_padding: DimensionPadding { bottom: 0, top: 0 },
            pool_aliases: Vec::new(),
            liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
        };
        let mut rng = LegacyRandom::from_seed(1);
        let mut get_height = |_: i32, _: i32| 64;

        let assembly = assemble(
            &config,
            &mut rng,
            0,
            0,
            &pools,
            &templates,
            &alias_map,
            &mut get_height,
            -64,
            320,
        )
        .expect("feature pool element exposes vanilla's synthetic bottom jigsaw");

        assert_eq!(assembly.pieces.len(), 1);
    }
}
