use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{Registry, vanilla_blocks, vanilla_entities};
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockStateId, BoundingBox, Direction};

use crate::entity::{entities::RawEntity, next_entity_id};
use crate::worldgen::region::WorldGenRegion;
use steel_worldgen::structure::ocean_monument::{
    OceanMonumentChildPiece, OceanMonumentChildPieceKind, OceanMonumentPieceData,
    OceanMonumentRoomData,
};

use super::StructurePiecePlacer;
use super::scattered_feature::ScatteredFeaturePlacer;

impl StructurePiecePlacer {
    pub(super) fn place_ocean_monument_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounding_box: BoundingBox,
        orientation: Option<Direction>,
        data: &mut OceanMonumentPieceData,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
    ) -> bool {
        {
            let mut building_box = bounding_box;
            let mut placer =
                ScatteredFeaturePlacer::new(region, registry, &mut building_box, orientation, clip);
            place_monument_building_shell(&mut placer);
        }

        for child in &data.child_pieces {
            if !child.bounding_box.intersects(clip) {
                continue;
            }

            let mut child_box = child.bounding_box;
            let mut placer =
                ScatteredFeaturePlacer::new(region, registry, &mut child_box, orientation, clip);
            place_child_piece(&mut placer, child, random);
        }

        true
    }
}

fn place_child_piece(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    child: &OceanMonumentChildPiece,
    random: &mut WorldgenRandom,
) {
    match &child.kind {
        OceanMonumentChildPieceKind::EntryRoom { room } => place_entry_room(placer, *room),
        OceanMonumentChildPieceKind::CoreRoom => place_core_room(placer),
        OceanMonumentChildPieceKind::DoubleXRoom { west, east } => {
            place_double_x_room(placer, *west, *east);
        }
        OceanMonumentChildPieceKind::DoubleXYRoom {
            west,
            east,
            west_up,
            east_up,
        } => place_double_xy_room(placer, *west, *east, *west_up, *east_up),
        OceanMonumentChildPieceKind::DoubleYRoom { room, above } => {
            place_double_y_room(placer, *room, *above);
        }
        OceanMonumentChildPieceKind::DoubleYZRoom {
            south,
            north,
            south_up,
            north_up,
        } => place_double_yz_room(placer, *south, *north, *south_up, *north_up),
        OceanMonumentChildPieceKind::DoubleZRoom { south, north } => {
            place_double_z_room(placer, *south, *north);
        }
        OceanMonumentChildPieceKind::SimpleRoom { room, main_design } => {
            place_simple_room(placer, random, *room, *main_design);
        }
        OceanMonumentChildPieceKind::SimpleTopRoom { room } => {
            place_simple_top_room(placer, random, *room);
        }
        OceanMonumentChildPieceKind::WingRoom { main_design } => {
            place_wing_room(placer, *main_design);
        }
        OceanMonumentChildPieceKind::Penthouse => place_penthouse(placer),
    }
}

mod shell;

use shell::place_monument_building_shell;

mod room_core;

use room_core::{place_core_room, place_entry_room};

mod room_double;

use room_double::{
    place_double_x_room, place_double_xy_room, place_double_y_room, place_double_yz_room,
    place_double_z_room,
};

mod room_simple;

use room_simple::{place_simple_room, place_simple_top_room};

mod room_special;

use room_special::{place_penthouse, place_wing_room};

mod helpers;

use helpers::{
    base_black, base_gray, base_light, dot_deco, generate_box_on_fill_only, generate_default_floor,
    generate_water_box, lamp, open, spawn_elder,
};
