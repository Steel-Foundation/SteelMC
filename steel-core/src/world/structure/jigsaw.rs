//! Jigsaw structure assembly engine.
//!
//! Ports vanilla's `JigsawPlacement` BFS algorithm. Given a start pool and
//! configuration, assembles structure pieces by connecting jigsaw blocks.
//!
//! The assembly determines which pieces exist and their bounding boxes.
//! Actual block placement from templates is handled separately.

use rustc_hash::FxHashMap;
use steel_registry::structure_set::{JigsawConfig, PoolAlias, StartHeight};
use steel_registry::template_pool::{
    JigsawOrientation, JointType, PoolElement, Projection, TemplateData, TemplatePoolData,
};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier, Rotation};

/// A placed piece produced by jigsaw assembly.
#[derive(Debug, Clone)]
pub struct PlacedPiece {
    /// Which pool element this piece came from.
    pub element_index: usize,
    /// The template location (for Single/LegacySingle elements).
    pub template_location: Option<Identifier>,
    /// World position of the piece's origin.
    pub position: (i32, i32, i32),
    /// Rotation applied to this piece.
    pub rotation: Rotation,
    /// World-space bounding box (template-sized, used for beardifier and world save).
    pub bounding_box: BoundingBox,
    /// Assembly-time bounding box, potentially expanded vertically by the expansion
    /// hack to reserve space for children. Used for `is_inside` and collision checks
    /// during assembly only — not persisted.
    pub assembly_bb: BoundingBox,
    /// Ground level delta for Beardifier terrain adaptation.
    pub ground_level_delta: i32,
    /// Projection mode (rigid or terrain matching).
    pub projection: Projection,
    /// Generation depth (distance from start piece in BFS tree).
    pub depth: i32,
    /// Junctions connecting this piece to neighbors.
    pub junctions: Vec<JigsawJunction>,
}

/// A junction between two jigsaw pieces, used for terrain adaptation.
#[derive(Debug, Clone)]
pub struct JigsawJunction {
    /// World X of the junction point.
    pub source_x: i32,
    /// Ground-adjusted Y of the junction.
    pub source_ground_y: i32,
    /// World Z of the junction point.
    pub source_z: i32,
    /// Y delta between source and target pieces.
    pub delta_y: i32,
    /// Projection mode of the destination piece.
    pub dest_projection: Projection,
}

/// Resolves pool aliases for a specific structure instance.
pub fn resolve_aliases(
    aliases: &[PoolAlias],
    rng: &mut LegacyRandom,
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

/// Vanilla-matching shuffle (reverse Fisher-Yates).
fn vanilla_shuffle<T>(list: &mut [T], rng: &mut LegacyRandom) {
    for i in (1..list.len()).rev() {
        let j = rng.next_i32_bounded((i + 1) as i32) as usize;
        list.swap(i, j);
    }
}

/// Gets the template location from a pool element.
const fn element_location(element: &PoolElement) -> Option<&Identifier> {
    match element {
        PoolElement::Single { location, .. } | PoolElement::LegacySingle { location, .. } => {
            Some(location)
        }
        _ => None,
    }
}

/// Gets shuffled jigsaws for a pool element at a given position and rotation.
///
/// Returns the jigsaws with their positions transformed by rotation, sorted
/// by `selection_priority` (descending), then shuffled within equal priorities.
fn get_shuffled_jigsaws(
    element: &PoolElement,
    templates: &FxHashMap<Identifier, TemplateData>,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<TransformedJigsaw> {
    let location = match element_location(element) {
        Some(loc) => loc,
        None => {
            // Feature/Empty elements: synthetic jigsaw at origin facing down
            return vec![TransformedJigsaw {
                pos: (0, 0, 0),
                orientation: JigsawOrientation::DownSouth,
                name: Identifier::new_static("minecraft", "bottom"),
                target: Identifier::new_static("minecraft", "empty"),
                pool: Identifier::new_static("minecraft", "empty"),
                joint: JointType::Rollable,
                selection_priority: 0,
                placement_priority: 0,
            }];
        }
    };

    let Some(template) = templates.get(location) else {
        return vec![];
    };

    let mut jigsaws: Vec<TransformedJigsaw> = template
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
                name: j.name.clone(),
                target: j.target.clone(),
                pool: j.pool.clone(),
                joint: j.joint,
                selection_priority: j.selection_priority,
                placement_priority: j.placement_priority,
            }
        })
        .collect();

    // Shuffle first, then stable-sort by selection_priority descending
    vanilla_shuffle(&mut jigsaws, rng);
    jigsaws.sort_by(|a, b| b.selection_priority.cmp(&a.selection_priority));

    jigsaws
}

/// A jigsaw block with its position transformed by rotation.
struct TransformedJigsaw {
    pos: (i32, i32, i32),
    orientation: JigsawOrientation,
    name: Identifier,
    target: Identifier,
    pool: Identifier,
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
            let loc = match element {
                PoolElement::Single { location, .. }
                | PoolElement::LegacySingle { location, .. } => location,
                _ => return None,
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
fn element_bounding_box(
    element: &PoolElement,
    templates: &FxHashMap<Identifier, TemplateData>,
    pos_x: i32,
    pos_y: i32,
    pos_z: i32,
    rotation: Rotation,
) -> Option<BoundingBox> {
    if let PoolElement::Feature { .. } = element {
        Some(BoundingBox::new(pos_x, pos_y, pos_z, pos_x, pos_y, pos_z))
    } else {
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

/// Builds the expanded weighted template list from a pool.
///
/// Vanilla's `StructureTemplatePool.getShuffledTemplates` expands weights
/// then shuffles.
fn get_shuffled_templates<'a>(
    pool: &'a TemplatePoolData,
    rng: &mut LegacyRandom,
) -> Vec<&'a PoolElement> {
    let mut expanded: Vec<&PoolElement> = Vec::new();
    for (element, weight) in &pool.elements {
        for _ in 0..*weight {
            expanded.push(element);
        }
    }
    vanilla_shuffle(&mut expanded, rng);
    expanded
}

/// Picks a random template from a pool (weighted).
///
/// Vanilla's `StructureTemplatePool.getRandomTemplate`.
fn get_random_template<'a>(pool: &'a TemplatePoolData, rng: &mut LegacyRandom) -> &'a PoolElement {
    let mut expanded: Vec<&PoolElement> = Vec::new();
    for (element, weight) in &pool.elements {
        for _ in 0..*weight {
            expanded.push(element);
        }
    }
    if expanded.is_empty() {
        // Return empty element sentinel
        static EMPTY: PoolElement = PoolElement::Empty;
        return &EMPTY;
    }
    let idx = rng.next_i32_bounded(expanded.len() as i32) as usize;
    expanded[idx]
}

/// Collision domain tracking available space for piece placement.
///
/// Vanilla uses `MutableObject<VoxelShape>` to track free space hierarchically:
/// each queued piece carries a reference to its parent's collision context.
/// Internal children share the source piece's internal free space; external
/// children share the parent's context. For integer-aligned bounding boxes,
/// tracking a constraint + occupied list is equivalent to `VoxelShape` subtraction.
struct FreeSpace {
    /// The outer boundary of this domain.
    constraint: BoundingBox,
    /// Bounding boxes of pieces placed within this domain.
    occupied: Vec<BoundingBox>,
}

impl FreeSpace {
    /// Checks if a bounding box fits within this domain without collisions.
    fn collides(&self, candidate: &BoundingBox) -> bool {
        // Must fit within constraint
        if candidate.min_x < self.constraint.min_x
            || candidate.max_x > self.constraint.max_x
            || candidate.min_y < self.constraint.min_y
            || candidate.max_y > self.constraint.max_y
            || candidate.min_z < self.constraint.min_z
            || candidate.max_z > self.constraint.max_z
        {
            return true;
        }
        // Must not intersect any placed piece in this domain
        for placed in &self.occupied {
            if candidate.intersects(placed) {
                return true;
            }
        }
        false
    }
}

/// Result of a successful jigsaw assembly.
pub struct AssemblyResult {
    /// The placed pieces.
    pub pieces: Vec<PlacedPiece>,
    /// The biome check position (centerX, centerY, centerZ from the `GenerationStub`).
    pub biome_check_pos: (i32, i32, i32),
}

/// Assembles a jigsaw structure from the given configuration.
///
/// Returns the assembly result, or `None` if assembly fails
/// (empty start pool, dimension padding violation, etc.).
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
    // Sample start height
    let start_y = match &config.start_height {
        StartHeight::Constant(y) => *y,
        StartHeight::Uniform { min, max } => rng.next_i32_between(*min, *max),
    };

    let start_x = chunk_x * 16;
    let start_z = chunk_z * 16;

    // Pick random rotation
    let center_rotation = Rotation::get_random(rng);

    // Resolve start pool through aliases
    let start_pool_key = alias_map
        .get(&config.start_pool)
        .unwrap_or(&config.start_pool);
    let start_pool = pools.get(start_pool_key)?;

    // Pick random start template
    let center_element = get_random_template(start_pool, rng);
    if center_element.is_empty() {
        return None;
    }

    // Handle start_jigsaw_name anchor
    let (anchor_offset_x, anchor_offset_y, anchor_offset_z) =
        if let Some(ref jigsaw_name) = config.start_jigsaw_name {
            // Find the named jigsaw in the start piece
            let jigsaws = get_shuffled_jigsaws(center_element, templates, center_rotation, rng);
            let found = jigsaws.iter().find(|j| j.name == *jigsaw_name);
            match found {
                Some(j) => (j.pos.0, j.pos.1, j.pos.2),
                None => return None, // Named jigsaw not found
            }
        } else {
            (0, 0, 0)
        };

    // Adjusted position: move piece so anchor aligns with start position
    let adjusted_x = start_x - anchor_offset_x;
    let adjusted_y = start_y - anchor_offset_y;
    let adjusted_z = start_z - anchor_offset_z;

    // Compute center piece bounding box
    let center_bb = element_bounding_box(
        center_element,
        templates,
        adjusted_x,
        adjusted_y,
        adjusted_z,
        center_rotation,
    )?;

    // Height projection
    let bottom_y = if let Some(ref _heightmap) = config.project_start_to_heightmap {
        let center_bx = i32::midpoint(center_bb.min_x, center_bb.max_x);
        let center_bz = i32::midpoint(center_bb.min_z, center_bb.max_z);
        // Vanilla: start_y + getFirstFreeHeight(...)
        let surface = get_height(center_bx, center_bz);
        start_y + surface
    } else {
        adjusted_y
    };

    // Move center piece to projected height
    let ground_level_delta = center_element.projection().ground_level_delta();
    let old_ground_y = center_bb.min_y + ground_level_delta;
    let dy = bottom_y - old_ground_y;
    let center_bb = BoundingBox::new(
        center_bb.min_x,
        center_bb.min_y + dy,
        center_bb.min_z,
        center_bb.max_x,
        center_bb.max_y + dy,
        center_bb.max_z,
    );
    let adjusted_y = adjusted_y + dy;

    // Dimension padding check
    let padding = &config.dimension_padding;
    // Vanilla uses getMaxY() = minY + height - 1 (inclusive), so max_y - 1 here
    if center_bb.min_y < min_y + padding.bottom || center_bb.max_y > max_y - 1 - padding.top {
        return None;
    }

    // Create center piece
    let mut pieces = vec![PlacedPiece {
        element_index: 0,
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

    // Compute biome check position (vanilla's GenerationStub position)
    let center_stub_x = i32::midpoint(center_bb.min_x, center_bb.max_x);
    let center_stub_z = i32::midpoint(center_bb.min_z, center_bb.max_z);
    let center_stub_y = bottom_y + anchor_offset_y;
    let biome_check_pos = (center_stub_x, center_stub_y, center_stub_z);

    if config.max_depth <= 0 {
        return Some(AssemblyResult {
            pieces,
            biome_check_pos,
        });
    }

    // Create constraint bounding box — vanilla centers on (centerX, centerY, centerZ),
    // NOT on the BB corners. Uses AABB with +1 on max side, but for integer BB collision
    // the effective range is [center - maxDist, center + maxDist].
    let max_dist = config.max_distance_from_center;
    let center_y = center_stub_y;
    let constraint_bb = BoundingBox::new(
        center_stub_x - max_dist,
        (center_y - max_dist).max(min_y + config.dimension_padding.bottom),
        center_stub_z - max_dist,
        center_stub_x + max_dist,
        (center_y + max_dist).min(max_y - config.dimension_padding.top),
        center_stub_z + max_dist,
    );

    // Collision domains — vanilla's hierarchical free space tracking.
    // Index 0 is the global context (constraint minus placed pieces).
    let mut free_spaces: Vec<FreeSpace> = vec![FreeSpace {
        constraint: constraint_bb,
        occupied: vec![center_bb],
    }];

    // BFS queue: (piece_index, depth, placement_priority, context_idx)
    let mut queue: Vec<(usize, i32, i32, usize)> = Vec::new();

    // Seed queue with center piece (context 0 = global)
    try_placing_children(
        0, // center piece index
        0, // depth
        0, // context_idx (global)
        config,
        pools,
        templates,
        alias_map,
        &mut pieces,
        &mut free_spaces,
        &mut queue,
        rng,
        get_height,
    );

    // BFS loop
    while !queue.is_empty() {
        // Sort by priority (higher first), stable for insertion order
        queue.sort_by(|a, b| b.2.cmp(&a.2));
        let (piece_idx, depth, _priority, context_idx) = queue.remove(0);

        try_placing_children(
            piece_idx,
            depth,
            context_idx,
            config,
            pools,
            templates,
            alias_map,
            &mut pieces,
            &mut free_spaces,
            &mut queue,
            rng,
            get_height,
        );
    }

    Some(AssemblyResult {
        pieces,
        biome_check_pos,
    })
}

/// Tries to place children for a source piece.
///
/// For each jigsaw on the source piece, attempts to find a matching target
/// from the appropriate pool and place it.
///
/// `context_idx` is this piece's collision context in `free_spaces`:
/// - If this piece was placed externally, it's the parent's context
/// - If this piece was placed internally, it's the parent's internal free space
#[allow(clippy::too_many_arguments)]
fn try_placing_children(
    source_idx: usize,
    depth: i32,
    context_idx: usize,
    config: &JigsawConfig,
    pools: &FxHashMap<Identifier, TemplatePoolData>,
    templates: &FxHashMap<Identifier, TemplateData>,
    alias_map: &FxHashMap<Identifier, Identifier>,
    pieces: &mut Vec<PlacedPiece>,
    free_spaces: &mut Vec<FreeSpace>,
    queue: &mut Vec<(usize, i32, i32, usize)>,
    rng: &mut LegacyRandom,
    get_height: &mut dyn FnMut(i32, i32) -> i32,
) {
    let source_piece = pieces[source_idx].clone();
    let source_element_loc = source_piece.template_location.as_ref();
    let source_bb = source_piece.assembly_bb;
    let source_box_y = source_bb.min_y;
    let source_rigid = source_piece.projection == Projection::Rigid;

    // Vanilla's sourceFree: tracks free space inside source piece for internal
    // placements. Lazily initialized on first internal placement (matching
    // vanilla's `MutableObject<VoxelShape>` that starts null).
    let mut internal_ctx_idx: Option<usize> = None;

    // Get the pool element to retrieve jigsaws
    let source_pool_element = source_element_loc.and_then(|loc| {
        // Reconstruct element type — for jigsaw we only need Single/LegacySingle
        templates.get(loc).map(|_| loc)
    });

    let Some(source_loc) = source_pool_element else {
        return;
    };

    // Get shuffled jigsaws from source piece
    let source_jigsaws = {
        let Some(template) = templates.get(source_loc) else {
            return;
        };
        let mut jigsaws: Vec<TransformedJigsaw> = template
            .jigsaws
            .iter()
            .map(|j| {
                let (tx, ty, tz) = source_piece
                    .rotation
                    .transform_pos(j.pos[0], j.pos[1], j.pos[2], 0, 0);
                let orientation = j.orientation.rotate(source_piece.rotation);
                TransformedJigsaw {
                    pos: (
                        tx + source_piece.position.0,
                        ty + source_piece.position.1,
                        tz + source_piece.position.2,
                    ),
                    orientation,
                    name: j.name.clone(),
                    target: j.target.clone(),
                    pool: j.pool.clone(),
                    joint: j.joint,
                    selection_priority: j.selection_priority,
                    placement_priority: j.placement_priority,
                }
            })
            .collect();
        vanilla_shuffle(&mut jigsaws, rng);
        jigsaws.sort_by(|a, b| b.selection_priority.cmp(&a.selection_priority));
        jigsaws
    };

    // For each source jigsaw, try to place one child
    'source_jigsaw: for source_jigsaw in &source_jigsaws {
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
            .get(&source_jigsaw.pool)
            .unwrap_or(&source_jigsaw.pool);
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

        // Build candidate list
        let mut candidates: Vec<&PoolElement> = Vec::new();
        if depth != config.max_depth
            && let Some(pool) = target_pool {
                candidates.extend(get_shuffled_templates(pool, rng));
            }
        if let Some(fallback) = fallback_pool {
            candidates.extend(get_shuffled_templates(fallback, rng));
        }

        let placement_priority = source_jigsaw.placement_priority;

        // Track source jigsaw base height (lazy, for terrain matching)
        let mut source_jigsaw_base_height: Option<i32> = None;

        // Try each candidate
        for candidate_element in &candidates {
            if candidate_element.is_empty() {
                break;
            }

            let candidate_projection = candidate_element.projection();
            let candidate_rigid = candidate_projection == Projection::Rigid;

            // Try each rotation
            let rotations = Rotation::get_shuffled(rng);
            for candidate_rotation in rotations {
                let candidate_jigsaws =
                    get_shuffled_jigsaws(candidate_element, templates, candidate_rotation, rng);

                let _candidate_bb_at_origin =
                    element_bounding_box(candidate_element, templates, 0, 0, 0, candidate_rotation);

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
                                    let (fdx2, fdy2, fdz2) = front.offset();
                                    let front_pos = (rx + fdx2, ry + fdy2, rz + fdz2);
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

                // Try each target jigsaw
                for target_jigsaw in &candidate_jigsaws {
                    if !can_attach(source_jigsaw, target_jigsaw) {
                        continue;
                    }

                    let target_jigsaw_local = target_jigsaw.pos;

                    // Compute raw target position
                    let raw_target_x = target_jigsaw_world.0 - target_jigsaw_local.0;
                    let raw_target_z = target_jigsaw_world.2 - target_jigsaw_local.2;

                    // Compute raw bounding box at that position
                    let Some(raw_bb) = element_bounding_box(
                        candidate_element,
                        templates,
                        raw_target_x,
                        0,
                        raw_target_z,
                        candidate_rotation,
                    ) else {
                        continue;
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
                        source_piece.ground_level_delta - delta_y
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
                            + source_piece.ground_level_delta,
                        source_z: target_jigsaw_world.2,
                        delta_y,
                        dest_projection: candidate_projection,
                    });

                    let new_piece_idx = pieces.len();

                    // Create target piece
                    let mut target_piece = PlacedPiece {
                        element_index: 0,
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
                        dest_projection: source_piece.projection,
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
