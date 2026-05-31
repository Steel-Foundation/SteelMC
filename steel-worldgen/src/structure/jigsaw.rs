//! Jigsaw assembly. Ports vanilla's `JigsawPlacement` BFS: connects pieces via
//! jigsaw blocks given a start pool + config. Produces typed piece state;
//! block placement runs in a later worldgen stage.

use std::{cmp::Reverse, mem};

use rustc_hash::FxHashMap;
use steel_registry::structure::{
    JigsawConfig, LiquidSettingsData, PoolAlias, StartHeight, StructureData,
};
use steel_registry::template_pool::{
    JigsawOrientation, JointType, PoolElement, Projection, TemplateData, TemplatePoolData,
};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{PositionalRandom, Random};
use steel_utils::{BoundingBox, Identifier, Rotation};

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
    pub position: (i32, i32, i32),
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
    pub position: (i32, i32, i32),
    /// Template rotation.
    pub rotation: Rotation,
    /// Liquid handling mode for block placement.
    pub liquid_settings: LiquidSettingsData,
}

/// Junction between two jigsaw pieces (terrain adaptation).
#[derive(Debug, Clone)]
pub struct JigsawJunction {
    /// World X.
    pub source_x: i32,
    /// Ground-adjusted Y.
    pub source_ground_y: i32,
    /// World Z.
    pub source_z: i32,
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

type JigsawTransformCache<'a> = FxHashMap<Identifier, [Option<CachedTemplateJigsaws<'a>>; 4]>;
type PoolTemplateCache<'a> = FxHashMap<Identifier, Vec<&'a PoolElement>>;

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

fn shuffle_jigsaws(jigsaws: &mut [TransformedJigsaw<'_>], rng: &mut LegacyRandom) {
    vanilla_shuffle(jigsaws, rng);
    let priorities = descending_priorities(jigsaws.iter().map(|jigsaw| jigsaw.selection_priority));
    order_jigsaws_by_priorities(jigsaws, &priorities);
}

fn shuffled_jigsaw_indices(
    cached: &CachedTemplateJigsaws<'_>,
    rng: &mut LegacyRandom,
) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..cached.jigsaws.len()).collect();
    vanilla_shuffle(&mut indices, rng);
    order_jigsaw_indices_by_priorities(cached, &mut indices);
    indices
}

fn descending_priorities(priorities: impl Iterator<Item = i32>) -> Vec<i32> {
    let mut unique = Vec::new();
    for priority in priorities {
        if !unique.contains(&priority) {
            unique.push(priority);
        }
    }
    if unique.len() > 1 {
        unique.sort_unstable_by_key(|priority| Reverse(*priority));
    }
    unique
}

fn order_jigsaws_by_priorities(jigsaws: &mut [TransformedJigsaw<'_>], priorities: &[i32]) {
    if priorities.len() <= 1 {
        return;
    }

    let original = jigsaws.to_vec();
    let mut out = Vec::with_capacity(original.len());
    for &priority in priorities {
        out.extend(
            original
                .iter()
                .copied()
                .filter(|jigsaw| jigsaw.selection_priority == priority),
        );
    }
    jigsaws.copy_from_slice(&out);
}

fn order_jigsaw_indices_by_priorities(
    cached: &CachedTemplateJigsaws<'_>,
    indices: &mut Vec<usize>,
) {
    if cached.priorities_desc.len() <= 1 {
        return;
    }

    let original = mem::take(indices);
    let mut ordered = Vec::with_capacity(original.len());
    for &priority in &cached.priorities_desc {
        ordered.extend(
            original
                .iter()
                .copied()
                .filter(|&idx| cached.jigsaws[idx].selection_priority == priority),
        );
    }
    *indices = ordered;
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

/// Gets shuffled jigsaws for a pool element at a given position and rotation.
///
/// Returns the jigsaws with their positions transformed by rotation, sorted
/// by `selection_priority` (descending), then shuffled within equal priorities.
fn get_shuffled_jigsaws<'a>(
    element: &PoolElement,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<TransformedJigsaw<'a>> {
    let Some(location) = element_location(element) else {
        // Feature/Empty elements: synthetic jigsaw at origin facing down
        let mut jigsaws = vec![TransformedJigsaw {
            pos: (0, 0, 0),
            orientation: JigsawOrientation::DownSouth,
            name: &SYNTHETIC_BOTTOM_JIGSAW,
            target: &SYNTHETIC_EMPTY_POOL,
            pool: &SYNTHETIC_EMPTY_POOL,
            joint: JointType::Rollable,
            selection_priority: 0,
            placement_priority: 0,
        }];
        shuffle_jigsaws(&mut jigsaws, rng);
        return jigsaws;
    };

    let Some(template) = templates.get(location) else {
        return vec![];
    };

    let mut jigsaws = transform_template_jigsaws(template, rotation);
    shuffle_jigsaws(&mut jigsaws, rng);
    jigsaws
}

fn get_cached_shuffled_jigsaws<'a>(
    element: &PoolElement,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    cache: &mut JigsawTransformCache<'a>,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<TransformedJigsaw<'a>> {
    let Some(location) = element_location(element) else {
        let mut jigsaws = vec![TransformedJigsaw {
            pos: (0, 0, 0),
            orientation: JigsawOrientation::DownSouth,
            name: &SYNTHETIC_BOTTOM_JIGSAW,
            target: &SYNTHETIC_EMPTY_POOL,
            pool: &SYNTHETIC_EMPTY_POOL,
            joint: JointType::Rollable,
            selection_priority: 0,
            placement_priority: 0,
        }];
        shuffle_jigsaws(&mut jigsaws, rng);
        return jigsaws;
    };

    get_cached_shuffled_template_jigsaws(location, templates, cache, rotation, rng)
}

fn get_cached_shuffled_template_jigsaws<'a>(
    location: &Identifier,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    cache: &mut JigsawTransformCache<'a>,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<TransformedJigsaw<'a>> {
    let cached = get_cached_template_jigsaws(location, templates, cache, rotation);
    let mut jigsaws = cached.jigsaws.clone();
    vanilla_shuffle(&mut jigsaws, rng);
    order_jigsaws_by_priorities(&mut jigsaws, &cached.priorities_desc);
    jigsaws
}

fn get_cached_template_jigsaws<'cache, 'a>(
    location: &Identifier,
    templates: &'a FxHashMap<Identifier, TemplateData>,
    cache: &'cache mut JigsawTransformCache<'a>,
    rotation: Rotation,
) -> &'cache CachedTemplateJigsaws<'a> {
    let idx = rotation_index(rotation);
    let by_rotation = cache
        .entry(location.clone())
        .or_insert_with(|| [(); 4].map(|()| None));
    by_rotation[idx].get_or_insert_with(|| {
        templates.get(location).map_or_else(
            || CachedTemplateJigsaws {
                jigsaws: Vec::new(),
                priorities_desc: Vec::new(),
            },
            |template| {
                let jigsaws = transform_template_jigsaws(template, rotation);
                let priorities_desc =
                    descending_priorities(jigsaws.iter().map(|jigsaw| jigsaw.selection_priority));
                CachedTemplateJigsaws {
                    jigsaws,
                    priorities_desc,
                }
            },
        )
    })
}

fn transform_template_jigsaws(
    template: &TemplateData,
    rotation: Rotation,
) -> Vec<TransformedJigsaw<'_>> {
    template
        .jigsaws
        .iter()
        .map(|j| {
            // Transform position by rotation (pivot = 0,0,0)
            let (tx, ty, tz) = rotation.transform_pos(j.pos[0], j.pos[1], j.pos[2], 0, 0);
            // Rotate orientation
            let orientation = j.orientation.rotate(rotation);
            TransformedJigsaw {
                pos: (tx, ty, tz),
                orientation,
                name: &j.name,
                target: &j.target,
                pool: &j.pool,
                joint: j.joint,
                selection_priority: j.selection_priority,
                placement_priority: j.placement_priority,
            }
        })
        .collect()
}

struct CachedTemplateJigsaws<'a> {
    jigsaws: Vec<TransformedJigsaw<'a>>,
    priorities_desc: Vec<i32>,
}

/// A jigsaw block with its position transformed by rotation.
#[derive(Clone, Copy)]
struct TransformedJigsaw<'a> {
    pos: (i32, i32, i32),
    orientation: JigsawOrientation,
    name: &'a Identifier,
    target: &'a Identifier,
    pool: &'a Identifier,
    joint: JointType,
    selection_priority: i32,
    placement_priority: i32,
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

/// Checks if two jigsaws can connect.
///
/// Vanilla's `JigsawBlock.canAttach`: opposite facing, name match, joint compatibility.
fn can_attach(source: &TransformedJigsaw, target: &TransformedJigsaw) -> bool {
    let source_front = source.orientation.front_direction();
    let target_front = target.orientation.front_direction();

    // Fronts must be opposite
    if source_front != target_front.opposite() {
        return false;
    }

    // Names must match: source.target == target.name
    if source.target != target.name {
        return false;
    }

    // Joint compatibility: if aligned, tops must match
    if source.joint == JointType::Aligned {
        let source_top = source.orientation.top_direction();
        let target_top = target.orientation.top_direction();
        if source_top != target_top {
            return false;
        }
    }

    true
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
    pos_x: i32,
    pos_y: i32,
    pos_z: i32,
    rotation: Rotation,
) -> Option<BoundingBox> {
    match element {
        PoolElement::Feature { .. } => {
            Some(BoundingBox::new(pos_x, pos_y, pos_z, pos_x, pos_y, pos_z))
        }
        PoolElement::List { elements, .. } => {
            // Vanilla: encapsulating BB of all non-empty sub-elements
            let mut result: Option<BoundingBox> = None;
            for sub in elements {
                if let Some(sub_bb) =
                    element_bounding_box(sub, templates, pos_x, pos_y, pos_z, rotation)
                {
                    result = Some(match result {
                        Some(prev) => BoundingBox::new(
                            prev.min_x.min(sub_bb.min_x),
                            prev.min_y.min(sub_bb.min_y),
                            prev.min_z.min(sub_bb.min_z),
                            prev.max_x.max(sub_bb.max_x),
                            prev.max_y.max(sub_bb.max_y),
                            prev.max_z.max(sub_bb.max_z),
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
            Some(rotation.get_bounding_box(
                pos_x,
                pos_y,
                pos_z,
                template.size[0],
                template.size[1],
                template.size[2],
            ))
        }
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
        // `PoolElement::Empty` is a unit variant with no interior mutability,
        // so `&'static` sharing is sound.
        static EMPTY: PoolElement = PoolElement::Empty;
        return &EMPTY;
    }
    let idx = rng.next_i32_bounded(expanded.len() as i32) as usize;
    expanded[idx]
}

/// Hierarchical free-space tracker. Vanilla uses `MutableObject<VoxelShape>`
/// with subtraction; for integer-aligned BBs, `constraint + occupied` is
/// equivalent. Internal children share the source's internal free space;
/// external children share the parent's context.
struct FreeSpace {
    constraint: BoundingBox,
    occupied: Vec<BoundingBox>,
}

impl FreeSpace {
    fn collides(&self, candidate: &BoundingBox) -> bool {
        if candidate.min_x < self.constraint.min_x
            || candidate.max_x > self.constraint.max_x
            || candidate.min_y < self.constraint.min_y
            || candidate.max_y > self.constraint.max_y
            || candidate.min_z < self.constraint.min_z
            || candidate.max_z > self.constraint.max_z
        {
            return true;
        }
        self.occupied.iter().any(|p| candidate.intersects(p))
    }
}

/// Result of a successful jigsaw assembly.
pub struct AssemblyResult {
    /// The placed pieces.
    pub pieces: Vec<PlacedPiece>,
    /// The biome check position (centerX, centerY, centerZ from the `GenerationStub`).
    pub biome_check_pos: (i32, i32, i32),
}

/// Center-piece state produced before vanilla runs the `GenerationStub` piece builder.
struct StartedAssembly {
    pieces: Vec<PlacedPiece>,
    biome_check_pos: (i32, i32, i32),
}

/// Vanilla's `JigsawPlacement.addPieces` before the lazy `GenerationStub` child builder.
/// This must stop after the center piece so invalid start biomes do not assemble
/// full village/trial/ancient-city trees.
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

    let (anchor_offset_x, anchor_offset_y, anchor_offset_z) =
        if let Some(ref jigsaw_name) = config.start_jigsaw_name {
            let jigsaws = get_shuffled_jigsaws(center_element, templates, center_rotation, rng);
            let j = jigsaws.iter().find(|j| j.name == jigsaw_name)?;
            (j.pos.0, j.pos.1, j.pos.2)
        } else {
            (0, 0, 0)
        };

    // Move piece so anchor aligns with start position.
    let adjusted_x = start_x - anchor_offset_x;
    let adjusted_y = start_y - anchor_offset_y;
    let adjusted_z = start_z - anchor_offset_z;

    let center_bb = element_bounding_box(
        center_element,
        templates,
        adjusted_x,
        adjusted_y,
        adjusted_z,
        center_rotation,
    )?;

    let bottom_y = if config.project_start_to_heightmap.is_some() {
        let mid_x = java_center(center_bb.min_x, center_bb.max_x);
        let mid_z = java_center(center_bb.min_z, center_bb.max_z);
        start_y + get_height(mid_x, mid_z)
    } else {
        adjusted_y
    };

    // Move center piece to projected height.
    let ground_level_delta = center_element.projection().ground_level_delta();
    let dy = bottom_y - (center_bb.min_y + ground_level_delta);
    let center_bb = BoundingBox::new(
        center_bb.min_x,
        center_bb.min_y + dy,
        center_bb.min_z,
        center_bb.max_x,
        center_bb.max_y + dy,
        center_bb.max_z,
    );
    let adjusted_y = adjusted_y + dy;

    // Dimension padding. Vanilla's `getMaxY()` is inclusive (= minY + height - 1).
    let padding = &config.dimension_padding;
    if center_bb.min_y < min_y + padding.bottom || center_bb.max_y > max_y - 1 - padding.top {
        return None;
    }

    let pieces = vec![PlacedPiece {
        element: center_element.clone(),
        template_location: element_location(center_element).cloned(),
        position: (adjusted_x, adjusted_y, adjusted_z),
        rotation: center_rotation,
        bounding_box: center_bb,
        assembly_bb: center_bb,
        ground_level_delta,
        projection: center_element.projection(),
        depth: 0,
        junctions: Vec::new(),
    }];

    // GenerationStub center.
    let center_stub_x = java_center(center_bb.min_x, center_bb.max_x);
    let center_stub_z = java_center(center_bb.min_z, center_bb.max_z);
    let center_stub_y = bottom_y + anchor_offset_y;
    let biome_check_pos = (center_stub_x, center_stub_y, center_stub_z);

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
    let (center_stub_x, center_stub_y, center_stub_z) = biome_check_pos;

    // Vanilla centers the constraint on `(centerX, centerY, centerZ)`, NOT on BB
    // corners. Uses `+1` on the max side for AABB, but integer-BB collision with
    // `[center - maxDist, center + maxDist]` is equivalent.
    let max_dist = config.max_distance_from_center;
    let constraint_bb = BoundingBox::new(
        center_stub_x - max_dist,
        (center_stub_y - max_dist).max(min_y + config.dimension_padding.bottom),
        center_stub_z - max_dist,
        center_stub_x + max_dist,
        (center_stub_y + max_dist).min(max_y - 1 - config.dimension_padding.top),
        center_stub_z + max_dist,
    );

    // Index 0 = global collision context.
    let mut free_spaces: Vec<FreeSpace> = vec![FreeSpace {
        constraint: constraint_bb,
        occupied: vec![center_bb],
    }];
    let mut jigsaw_cache = JigsawTransformCache::default();
    let mut pool_template_cache = PoolTemplateCache::default();
    // (piece_index, depth, placement_priority, context_idx)
    let mut queue: Vec<(usize, i32, i32, usize)> = Vec::new();

    try_placing_children(
        0,
        0,
        0,
        config,
        pools,
        templates,
        alias_map,
        &mut jigsaw_cache,
        &mut pool_template_cache,
        &mut started.pieces,
        &mut free_spaces,
        &mut queue,
        rng,
        get_height,
    );

    while !queue.is_empty() {
        // Stable sort keeps insertion order within equal priorities.
        queue.sort_by_key(|entry| Reverse(entry.2));
        let (piece_idx, depth, _priority, context_idx) = queue.remove(0);
        try_placing_children(
            piece_idx,
            depth,
            context_idx,
            config,
            pools,
            templates,
            alias_map,
            &mut jigsaw_cache,
            &mut pool_template_cache,
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

        let (bx, by, bz) = started.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
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
                // Vanilla's `JigsawPlacement.tryPlacingChildren` mutates
                // `targetBB` in place via the expansion hack, then stores it on
                // `PoolElementStructurePiece`.
                bounding_box: piece.assembly_bb,
                // Vanilla's `PoolElementStructurePiece` constructor hardcodes
                // `genDepth = 0` for every jigsaw piece.
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
            position: assembly.biome_check_pos,
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
    jigsaw_cache: &mut JigsawTransformCache<'a>,
    pool_template_cache: &mut PoolTemplateCache<'a>,
    pieces: &mut Vec<PlacedPiece>,
    free_spaces: &mut Vec<FreeSpace>,
    queue: &mut Vec<(usize, i32, i32, usize)>,
    rng: &mut LegacyRandom,
    get_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    let (source_jigsaws, source_bb, source_projection, source_ground_level_delta) = {
        let source_piece = &pieces[source_idx];
        let mut jigsaws = get_cached_shuffled_jigsaws(
            &source_piece.element,
            templates,
            jigsaw_cache,
            source_piece.rotation,
            rng,
        );
        if jigsaws.is_empty() {
            return;
        }
        for jigsaw in &mut jigsaws {
            jigsaw.pos.0 += source_piece.position.0;
            jigsaw.pos.1 += source_piece.position.1;
            jigsaw.pos.2 += source_piece.position.2;
        }
        (
            jigsaws,
            source_piece.assembly_bb,
            source_piece.projection,
            source_piece.ground_level_delta,
        )
    };
    let source_box_y = source_bb.min_y;
    let source_rigid = source_projection == Projection::Rigid;

    // Vanilla's sourceFree: tracks free space inside source piece for internal
    // placements. Lazily initialized on first internal placement (matching
    // vanilla's `MutableObject<VoxelShape>` that starts null).
    let mut internal_ctx_idx: Option<usize> = None;
    let mut candidates: Vec<&PoolElement> = Vec::new();

    // For each source jigsaw, try to place one child
    'source_jigsaw: for source_jigsaw in &source_jigsaws {
        candidates.clear();
        let front = source_jigsaw.orientation.front_direction();
        let (fdx, fdy, fdz) = front.offset();
        let source_jigsaw_pos = source_jigsaw.pos;
        let target_jigsaw_world = (
            source_jigsaw_pos.0 + fdx,
            source_jigsaw_pos.1 + fdy,
            source_jigsaw_pos.2 + fdz,
        );

        let source_jigsaw_local_y = source_jigsaw_pos.1 - source_box_y;

        // Resolve target pool — vanilla always resolves fallback from the pool,
        // even if the main pool is empty
        let pool_key = alias_map
            .get(source_jigsaw.pool)
            .unwrap_or(source_jigsaw.pool);
        let raw_pool = pools.get(pool_key);
        let target_pool = raw_pool.filter(|p| !p.elements.is_empty());
        let fallback_pool = raw_pool
            .and_then(|p| pools.get(&p.fallback))
            .filter(|p| !p.elements.is_empty());

        // Determine whether target attaches inside source
        let attach_inside = source_bb.contains_xyz(
            target_jigsaw_world.0,
            target_jigsaw_world.1,
            target_jigsaw_world.2,
        );

        if depth != config.max_depth
            && let Some(pool) = target_pool
        {
            append_shuffled_templates_cached(pool, pool_template_cache, rng, &mut candidates);
        }
        if let Some(fallback) = fallback_pool {
            append_shuffled_templates_cached(fallback, pool_template_cache, rng, &mut candidates);
        }

        let placement_priority = source_jigsaw.placement_priority;

        // Track source jigsaw base height (lazy, for terrain matching)
        let mut source_jigsaw_base_height: Option<i32> = None;

        // Try each candidate
        for &candidate_element in &candidates {
            if candidate_element.is_empty() {
                break;
            }

            let candidate_projection = candidate_element.projection();
            let candidate_rigid = candidate_projection == Projection::Rigid;

            // Try each rotation
            let rotations = Rotation::get_shuffled(rng);
            for candidate_rotation in rotations {
                let fallback_jigsaws;
                let (candidate_jigsaws, candidate_jigsaw_order) =
                    if let Some(location) = element_location(candidate_element) {
                        let cached = get_cached_template_jigsaws(
                            location,
                            templates,
                            jigsaw_cache,
                            candidate_rotation,
                        );
                        (
                            cached.jigsaws.as_slice(),
                            shuffled_jigsaw_indices(cached, rng),
                        )
                    } else {
                        fallback_jigsaws = get_cached_shuffled_jigsaws(
                            candidate_element,
                            templates,
                            jigsaw_cache,
                            candidate_rotation,
                            rng,
                        );
                        (
                            fallback_jigsaws.as_slice(),
                            (0..fallback_jigsaws.len()).collect(),
                        )
                    };

                // Expansion hack: compute max child pool size for Y expansion.
                // Vanilla: getBoundingBox(manager, ZERO, rotation) uses default
                // StructurePlaceSettings with pivot=ZERO and mirror=NONE.
                // Jigsaw positions are also transformed with pivot=ZERO.
                let expand_to = if config.use_expansion_hack {
                    let hack_data =
                        element_location(candidate_element).and_then(|loc| templates.get(loc));
                    if let Some(template_data) = hack_data {
                        let hack_box = candidate_rotation.get_bounding_box(
                            0,
                            0,
                            0,
                            template_data.size[0],
                            template_data.size[1],
                            template_data.size[2],
                        );
                        if hack_box.max_y - hack_box.min_y < 16 {
                            template_data
                                .jigsaws
                                .iter()
                                .map(|j| {
                                    let (rx, ry, rz) = candidate_rotation
                                        .transform_pos(j.pos[0], j.pos[1], j.pos[2], 0, 0);
                                    let front =
                                        j.orientation.rotate(candidate_rotation).front_direction();
                                    let front_off = front.offset();
                                    let front_pos =
                                        (rx + front_off.0, ry + front_off.1, rz + front_off.2);
                                    if !hack_box.contains_xyz(front_pos.0, front_pos.1, front_pos.2)
                                    {
                                        return 0;
                                    }
                                    let child_pool_key = alias_map.get(&j.pool).unwrap_or(&j.pool);
                                    let child_pool_size = pools
                                        .get(child_pool_key)
                                        .map_or(0, |p| pool_max_y_size(p, templates));
                                    let child_fallback_size = pools
                                        .get(child_pool_key)
                                        .and_then(|p| pools.get(&p.fallback))
                                        .map_or(0, |p| pool_max_y_size(p, templates));
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

                // Try each target jigsaw
                for target_jigsaw_idx in candidate_jigsaw_order {
                    let target_jigsaw = candidate_jigsaws[target_jigsaw_idx];
                    if !can_attach(source_jigsaw, &target_jigsaw) {
                        continue;
                    }

                    let target_jigsaw_local = target_jigsaw.pos;

                    // Compute raw target position
                    let raw_target_x = target_jigsaw_world.0 - target_jigsaw_local.0;
                    let raw_target_z = target_jigsaw_world.2 - target_jigsaw_local.2;

                    // Compute raw bounding box at that position
                    let raw_bb = if let Some(bb) = candidate_bb_at_origin {
                        bb.moved(raw_target_x, 0, raw_target_z)
                    } else {
                        let Some(bb) = element_bounding_box(
                            candidate_element,
                            templates,
                            0,
                            0,
                            0,
                            candidate_rotation,
                        ) else {
                            continue;
                        };
                        candidate_bb_at_origin = Some(bb);
                        bb.moved(raw_target_x, 0, raw_target_z)
                    };

                    let target_jigsaw_local_y = target_jigsaw_local.1;

                    // Compute Y position
                    let delta_y = source_jigsaw_local_y - target_jigsaw_local_y + fdy;

                    let target_box_y = if source_rigid && candidate_rigid {
                        source_box_y + delta_y
                    } else {
                        let base_height = *source_jigsaw_base_height.get_or_insert_with(|| {
                            get_height(source_jigsaw_pos.0, source_jigsaw_pos.2)
                        });
                        base_height - target_jigsaw_local_y
                    };

                    let y_offset = target_box_y - raw_bb.min_y;
                    let candidate_bb = BoundingBox::new(
                        raw_bb.min_x,
                        raw_bb.min_y + y_offset,
                        raw_bb.min_z,
                        raw_bb.max_x,
                        raw_bb.max_y + y_offset,
                        raw_bb.max_z,
                    );
                    let target_position = (raw_target_x, raw_bb.min_y + y_offset, raw_target_z);

                    // Apply expansion hack: expand BB vertically to reserve space
                    // for potential children during assembly. The expanded BB is used
                    // for collision and is_inside checks; the original BB is stored
                    // in the piece for beardifier and world save.
                    let expanded_bb = if expand_to > 0 {
                        let new_size = (expand_to + 1).max(candidate_bb.max_y - candidate_bb.min_y);
                        BoundingBox::new(
                            candidate_bb.min_x,
                            candidate_bb.min_y,
                            candidate_bb.min_z,
                            candidate_bb.max_x,
                            candidate_bb.min_y + new_size,
                            candidate_bb.max_z,
                        )
                    } else {
                        candidate_bb
                    };

                    // Collision check — vanilla tracks free space hierarchically:
                    // internal children use sourceFree (this piece's internal space),
                    // external children use contextFree (parent's context).
                    let effective_ctx = if attach_inside {
                        *internal_ctx_idx.get_or_insert_with(|| {
                            free_spaces.push(FreeSpace {
                                constraint: source_bb,
                                occupied: Vec::new(),
                            });
                            free_spaces.len() - 1
                        })
                    } else {
                        context_idx
                    };

                    if free_spaces[effective_ctx].collides(&expanded_bb) {
                        continue;
                    }

                    // Success! Place this piece — subtract from the collision domain.
                    free_spaces[effective_ctx].occupied.push(expanded_bb);

                    // Compute ground level delta
                    let target_ground_level_delta = if candidate_rigid {
                        source_ground_level_delta - delta_y
                    } else {
                        candidate_projection.ground_level_delta()
                    };

                    // Compute junction Y
                    let junction_y = if source_rigid {
                        source_box_y + source_jigsaw_local_y
                    } else if candidate_rigid {
                        target_box_y + target_jigsaw_local_y
                    } else {
                        let base_height = *source_jigsaw_base_height.get_or_insert_with(|| {
                            get_height(source_jigsaw_pos.0, source_jigsaw_pos.2)
                        });
                        base_height + delta_y / 2
                    };

                    // Add junction to source piece
                    pieces[source_idx].junctions.push(JigsawJunction {
                        source_x: target_jigsaw_world.0,
                        source_ground_y: junction_y - source_jigsaw_local_y
                            + source_ground_level_delta,
                        source_z: target_jigsaw_world.2,
                        delta_y,
                        dest_projection: candidate_projection,
                    });

                    let new_piece_idx = pieces.len();

                    // Create target piece
                    let mut target_piece = PlacedPiece {
                        element: candidate_element.clone(),
                        template_location: element_location(candidate_element).cloned(),
                        position: target_position,
                        rotation: candidate_rotation,
                        bounding_box: candidate_bb,
                        assembly_bb: expanded_bb,
                        ground_level_delta: target_ground_level_delta,
                        projection: candidate_projection,
                        depth: depth + 1,
                        junctions: Vec::new(),
                    };

                    // Add junction to target piece
                    target_piece.junctions.push(JigsawJunction {
                        source_x: source_jigsaw_pos.0,
                        source_ground_y: junction_y - target_jigsaw_local_y
                            + target_ground_level_delta,
                        source_z: source_jigsaw_pos.2,
                        delta_y: -delta_y,
                        dest_projection: source_projection,
                    });

                    pieces.push(target_piece);

                    // Queue for further expansion if within depth limit.
                    // The child inherits the effective collision context:
                    // internal children get sourceFree, external get contextFree.
                    if depth < config.max_depth {
                        queue.push((new_piece_idx, depth + 1, placement_priority, effective_ctx));
                    }

                    // Break to next source jigsaw (one target per jigsaw)
                    continue 'source_jigsaw;
                }
            }
        }
    }
}
