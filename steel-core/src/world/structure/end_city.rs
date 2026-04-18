//! End city piece generation.
//!
//! Ports vanilla's `EndCityPieces`. Builds template-based pieces recursively
//! (base → towers → bridges → house towers/ships/fat towers) down to a
//! generation depth limit of 8.
//!
//! Only piece bounding boxes are produced; block placement is elsewhere.

use rustc_hash::FxHashMap;
use steel_registry::template_pool::TemplateData;
use steel_utils::density::DimensionNoises;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier, Rotation};

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{
    GenerationContext, GenerationStub, Structure, StructurePiece,
};

const MAX_GEN_DEPTH: i32 = 8;

/// An end-city piece described by template name, world template-position and rotation.
#[derive(Debug, Clone)]
pub struct EndCityPiece {
    /// Template name relative to `minecraft:end_city/`.
    pub template_name: String,
    /// World-space template-origin position.
    pub template_position: (i32, i32, i32),
    /// Piece rotation.
    pub rotation: Rotation,
    /// Generation depth tag (mutable — overwritten when its parent's
    /// `recursiveChildren` finishes).
    pub gen_depth: i32,
}

type Templates = FxHashMap<Identifier, TemplateData>;

fn template_size(templates: &Templates, name: &str) -> Option<[i32; 3]> {
    let id = Identifier::new("minecraft", format!("end_city/{name}"));
    templates.get(&id).map(|t| t.size)
}

fn piece_bb(templates: &Templates, piece: &EndCityPiece) -> BoundingBox {
    let size = template_size(templates, &piece.template_name)
        .unwrap_or_else(|| panic!("missing end_city template: {}", piece.template_name));
    piece.rotation.get_bounding_box(
        piece.template_position.0,
        piece.template_position.1,
        piece.template_position.2,
        size[0],
        size[1],
        size[2],
    )
}

/// Builds a child piece from a parent via vanilla's `addPiece` logic.
///
/// `calculateConnectedPosition(parent_settings, offset, child_settings, ZERO)`
/// with pivot=ZERO and mirror=NONE simplifies to `rotate(offset, parent.rotation)`,
/// because `transform(ZERO, _, _, ZERO) = ZERO`. The child's initial template
/// position is the parent's, then shifted by that rotated offset.
fn add_piece(
    parent: &EndCityPiece,
    offset: (i32, i32, i32),
    template_name: &str,
    rotation: Rotation,
) -> EndCityPiece {
    let (rx, ry, rz) =
        parent
            .rotation
            .transform_pos(offset.0, offset.1, offset.2, 0, 0);
    EndCityPiece {
        template_name: template_name.to_string(),
        template_position: (
            parent.template_position.0 + rx,
            parent.template_position.1 + ry,
            parent.template_position.2 + rz,
        ),
        rotation,
        gen_depth: 0,
    }
}

/// Internal generator state — tracks the one piece of shared state used in
/// vanilla: `TOWER_BRIDGE_GENERATOR.shipCreated`.
struct SharedState {
    ship_created: bool,
}

/// Produces child pieces for a given section-generator kind.
#[derive(Debug, Clone, Copy)]
enum SectionKind {
    HouseTower,
    Tower,
    TowerBridge,
    FatTower,
}

fn recursive_children(
    templates: &Templates,
    kind: SectionKind,
    gen_depth: i32,
    parent: &EndCityPiece,
    offset: (i32, i32, i32),
    pieces: &mut Vec<EndCityPiece>,
    shared: &mut SharedState,
    rng: &mut LegacyRandom,
) -> bool {
    if gen_depth > MAX_GEN_DEPTH {
        return false;
    }
    let mut child_pieces: Vec<EndCityPiece> = Vec::new();
    let ok = match kind {
        SectionKind::HouseTower => generate_house_tower(
            templates,
            gen_depth,
            parent,
            offset,
            &mut child_pieces,
            shared,
            rng,
        ),
        SectionKind::Tower => {
            generate_tower(templates, gen_depth, parent, &mut child_pieces, shared, rng)
        }
        SectionKind::TowerBridge => {
            generate_tower_bridge(templates, gen_depth, parent, &mut child_pieces, shared, rng)
        }
        SectionKind::FatTower => {
            generate_fat_tower(templates, gen_depth, parent, &mut child_pieces, shared, rng)
        }
    };
    if !ok {
        return false;
    }

    // Consume RNG for childTag and tag all children.
    let child_tag = rng.next_i32();
    let parent_tag = parent.gen_depth;
    let mut collision = false;
    for child in &mut child_pieces {
        child.gen_depth = child_tag;
        let child_bb = piece_bb(templates, child);
        for existing in pieces.iter() {
            if existing.gen_depth == parent_tag {
                continue;
            }
            let existing_bb = piece_bb(templates, existing);
            if existing_bb.intersects(&child_bb) {
                collision = true;
                break;
            }
        }
        if collision {
            break;
        }
    }

    if !collision {
        pieces.extend(child_pieces);
        true
    } else {
        false
    }
}

fn generate_house_tower(
    templates: &Templates,
    gen_depth: i32,
    parent: &EndCityPiece,
    offset: (i32, i32, i32),
    pieces: &mut Vec<EndCityPiece>,
    shared: &mut SharedState,
    rng: &mut LegacyRandom,
) -> bool {
    if gen_depth > MAX_GEN_DEPTH {
        return false;
    }
    let rotation = parent.rotation;
    let last = add_piece(parent, offset, "base_floor", rotation);
    pieces.push(last.clone());
    let num_floors = rng.next_i32_bounded(3);
    let mut last = pieces.last().unwrap().clone();
    if num_floors == 0 {
        let p = add_piece(&last, (-1, 4, -1), "base_roof", rotation);
        pieces.push(p.clone());
        let _ = p;
    } else if num_floors == 1 {
        let p = add_piece(&last, (-1, 0, -1), "second_floor_2", rotation);
        pieces.push(p.clone());
        last = p;
        let p = add_piece(&last, (-1, 8, -1), "second_roof", rotation);
        pieces.push(p.clone());
        last = p;
        recursive_children(
            templates,
            SectionKind::Tower,
            gen_depth + 1,
            &last,
            (0, 0, 0),
            pieces,
            shared,
            rng,
        );
    } else if num_floors == 2 {
        let p = add_piece(&last, (-1, 0, -1), "second_floor_2", rotation);
        pieces.push(p.clone());
        last = p;
        let p = add_piece(&last, (-1, 4, -1), "third_floor_2", rotation);
        pieces.push(p.clone());
        last = p;
        let p = add_piece(&last, (-1, 8, -1), "third_roof", rotation);
        pieces.push(p.clone());
        last = p;
        recursive_children(
            templates,
            SectionKind::Tower,
            gen_depth + 1,
            &last,
            (0, 0, 0),
            pieces,
            shared,
            rng,
        );
    }
    true
}

const TOWER_BRIDGES: [(Rotation, (i32, i32, i32)); 4] = [
    (Rotation::None, (1, -1, 0)),
    (Rotation::Clockwise90, (6, -1, 1)),
    (Rotation::CounterClockwise90, (0, -1, 5)),
    (Rotation::Clockwise180, (5, -1, 6)),
];

const FAT_TOWER_BRIDGES: [(Rotation, (i32, i32, i32)); 4] = [
    (Rotation::None, (4, -1, 0)),
    (Rotation::Clockwise90, (12, -1, 4)),
    (Rotation::CounterClockwise90, (0, -1, 8)),
    (Rotation::Clockwise180, (8, -1, 12)),
];

fn generate_tower(
    templates: &Templates,
    gen_depth: i32,
    parent: &EndCityPiece,
    pieces: &mut Vec<EndCityPiece>,
    shared: &mut SharedState,
    rng: &mut LegacyRandom,
) -> bool {
    let rotation = parent.rotation;
    let x_off = 3 + rng.next_i32_bounded(2);
    let z_off = 3 + rng.next_i32_bounded(2);
    let mut last = add_piece(parent, (x_off, -3, z_off), "tower_base", rotation);
    pieces.push(last.clone());
    let p = add_piece(&last, (0, 7, 0), "tower_piece", rotation);
    pieces.push(p.clone());
    last = p;

    let mut bridge_piece: Option<EndCityPiece> =
        if rng.next_i32_bounded(3) == 0 { Some(last.clone()) } else { None };
    let tower_height = 1 + rng.next_i32_bounded(3);
    for i in 0..tower_height {
        let p = add_piece(&last, (0, 4, 0), "tower_piece", rotation);
        pieces.push(p.clone());
        last = p;
        if i < tower_height - 1 && rng.next_bool() {
            bridge_piece = Some(last.clone());
        }
    }

    if let Some(bridge_anchor) = bridge_piece {
        for (rot_offset, offset) in TOWER_BRIDGES {
            if rng.next_bool() {
                let child_rot = rotation.then(rot_offset);
                let bridge_start = add_piece(&bridge_anchor, offset, "bridge_end", child_rot);
                pieces.push(bridge_start.clone());
                recursive_children(
                    templates,
                    SectionKind::TowerBridge,
                    gen_depth + 1,
                    &bridge_start,
                    (0, 0, 0),
                    pieces,
                    shared,
                    rng,
                );
            }
        }
        let p = add_piece(&last, (-1, 4, -1), "tower_top", rotation);
        pieces.push(p);
    } else if gen_depth != 7 {
        return recursive_children(
            templates,
            SectionKind::FatTower,
            gen_depth + 1,
            &last,
            (0, 0, 0),
            pieces,
            shared,
            rng,
        );
    } else {
        let p = add_piece(&last, (-1, 4, -1), "tower_top", rotation);
        pieces.push(p);
    }
    true
}

fn generate_tower_bridge(
    templates: &Templates,
    gen_depth: i32,
    parent: &EndCityPiece,
    pieces: &mut Vec<EndCityPiece>,
    shared: &mut SharedState,
    rng: &mut LegacyRandom,
) -> bool {
    let rotation = parent.rotation;
    let bridge_length = rng.next_i32_bounded(4) + 1;

    let first = add_piece(parent, (0, 0, -4), "bridge_piece", rotation);
    pieces.push(first);
    // Vanilla calls setGenDepth(-1) on the first bridge_piece so it's visible
    // as a "different batch" to sub-recursions' collision checks. It's later
    // overridden by this recursiveChildren's own childTag.
    if let Some(p) = pieces.last_mut() {
        p.gen_depth = -1;
    }

    let mut next_y = 0;
    let mut last = pieces.last().unwrap().clone();
    for _ in 0..bridge_length {
        if rng.next_bool() {
            let p = add_piece(&last, (0, next_y, -4), "bridge_piece", rotation);
            pieces.push(p.clone());
            last = p;
            next_y = 0;
        } else {
            if rng.next_bool() {
                let p = add_piece(&last, (0, next_y, -4), "bridge_steep_stairs", rotation);
                pieces.push(p.clone());
                last = p;
            } else {
                let p = add_piece(&last, (0, next_y, -8), "bridge_gentle_stairs", rotation);
                pieces.push(p.clone());
                last = p;
            }
            next_y = 4;
        }
    }

    if !shared.ship_created && rng.next_i32_bounded(10 - gen_depth) == 0 {
        let ship_x = -8 + rng.next_i32_bounded(8);
        let ship_z = -70 + rng.next_i32_bounded(10);
        let p = add_piece(&last, (ship_x, next_y, ship_z), "ship", rotation);
        pieces.push(p);
        shared.ship_created = true;
    } else if !recursive_children(
        templates,
        SectionKind::HouseTower,
        gen_depth + 1,
        &last,
        (-3, next_y + 1, -11),
        pieces,
        shared,
        rng,
    ) {
        // House-tower sub-recursion failed (collision). Vanilla returns false,
        // which causes the outer recursiveChildren to discard all pieces we
        // added here.
        return false;
    }

    let end_rot = rotation.then(Rotation::Clockwise180);
    let p = add_piece(&last, (4, next_y, 0), "bridge_end", end_rot);
    pieces.push(p);
    if let Some(p) = pieces.last_mut() {
        p.gen_depth = -1;
    }
    true
}

fn generate_fat_tower(
    templates: &Templates,
    gen_depth: i32,
    parent: &EndCityPiece,
    pieces: &mut Vec<EndCityPiece>,
    shared: &mut SharedState,
    rng: &mut LegacyRandom,
) -> bool {
    let rotation = parent.rotation;
    let mut last = add_piece(parent, (-3, 4, -3), "fat_tower_base", rotation);
    pieces.push(last.clone());
    let p = add_piece(&last, (0, 4, 0), "fat_tower_middle", rotation);
    pieces.push(p.clone());
    last = p;

    // `for (i = 0; i < 2 && random.nextInt(3) != 0; i++)` — each iteration
    // consumes one nextInt(3). If it returns 0, exit without body.
    for _ in 0..2 {
        if rng.next_i32_bounded(3) == 0 {
            break;
        }
        let p = add_piece(&last, (0, 8, 0), "fat_tower_middle", rotation);
        pieces.push(p.clone());
        last = p;

        for (rot_offset, offset) in FAT_TOWER_BRIDGES {
            if rng.next_bool() {
                let child_rot = rotation.then(rot_offset);
                let bridge_start = add_piece(&last, offset, "bridge_end", child_rot);
                pieces.push(bridge_start.clone());
                recursive_children(
                    templates,
                    SectionKind::TowerBridge,
                    gen_depth + 1,
                    &bridge_start,
                    (0, 0, 0),
                    pieces,
                    shared,
                    rng,
                );
            }
        }
    }
    let p = add_piece(&last, (-2, 8, -2), "fat_tower_top", rotation);
    pieces.push(p);
    true
}

/// Entry point. Mirrors vanilla's `EndCityPieces.startHouseTower`.
pub fn start_house_tower(
    templates: &Templates,
    origin: (i32, i32, i32),
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<EndCityPiece> {
    let mut pieces: Vec<EndCityPiece> = Vec::new();
    let mut shared = SharedState { ship_created: false };

    // Root: base_floor at origin (constructor, no offset math).
    let root = EndCityPiece {
        template_name: "base_floor".to_string(),
        template_position: origin,
        rotation,
        gen_depth: 0,
    };
    pieces.push(root.clone());

    let mut last = root;
    let p = add_piece(&last, (-1, 0, -1), "second_floor_1", rotation);
    pieces.push(p.clone());
    last = p;
    let p = add_piece(&last, (-1, 4, -1), "third_floor_1", rotation);
    pieces.push(p.clone());
    last = p;
    let p = add_piece(&last, (-1, 8, -1), "third_roof", rotation);
    pieces.push(p.clone());
    last = p;

    recursive_children(
        templates,
        SectionKind::Tower,
        1,
        &last,
        (0, 0, 0),
        &mut pieces,
        &mut shared,
        rng,
    );

    pieces
}

/// `Structure` impl — registered under `"minecraft:end_city"`.
///
/// Consumes rotation first (vanilla order), then computes a rotation-dependent
/// 5×5 lowestY probe. Rejects starts with `lowest < 60`. Biome-checks at the
/// final position. Pieces are produced by `start_house_tower`.
pub struct EndCityStructure;

impl<N: DimensionNoises> Structure<N> for EndCityStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let rotation = Rotation::get_random(rng);

        // Rotation-dependent 5×5 box offsets for the corner probes.
        let (off_x, off_z) = match rotation {
            Rotation::None => (5, 5),
            Rotation::Clockwise90 => (-5, 5),
            Rotation::Clockwise180 => (-5, -5),
            Rotation::CounterClockwise90 => (5, -5),
        };
        let bx = ctx.chunk_min_x + 7;
        let bz = ctx.chunk_min_z + 7;
        // End uses `base_height_full` because `preliminary_surface_level = 0`
        // in the end dimension — the capped variant would miss islands at Y=50+.
        let h0 = ctx.base_height_full(bx, bz, false) - 1;
        let h1 = ctx.base_height_full(bx, bz + off_z, false) - 1;
        let h2 = ctx.base_height_full(bx + off_x, bz, false) - 1;
        let h3 = ctx.base_height_full(bx + off_x, bz + off_z, false) - 1;
        let lowest = h0.min(h1).min(h2).min(h3);
        if lowest < 60 {
            return None;
        }

        let biome = ctx.biome_at(bx, lowest, bz);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let ec_pieces = start_house_tower(ctx.templates, (bx, lowest, bz), rotation, rng);
        let pieces = ec_pieces
            .into_iter()
            .map(|p| {
                let tmpl_id =
                    Identifier::new("minecraft", format!("end_city/{}", p.template_name));
                let size = ctx
                    .templates
                    .get(&tmpl_id)
                    .map(|t| t.size)
                    .unwrap_or([1, 1, 1]);
                let bb = p.rotation.get_bounding_box(
                    p.template_position.0,
                    p.template_position.1,
                    p.template_position.2,
                    size[0],
                    size[1],
                    size[2],
                );
                StructurePiece {
                    piece_type: Identifier::new_static("minecraft", "ecp"),
                    bounding_box: bb,
                    gen_depth: 0,
                    orientation: None,
                    nbt_data: Vec::new(),
                    ground_level_delta: 0,
                    junctions: Vec::new(),
                }
            })
            .collect();

        Some(GenerationStub {
            position: (bx, lowest, bz),
            pieces,
        })
    }
}
