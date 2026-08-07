use super::*;

#[test]
fn structure_persistence_filters_empty_starts_and_sorts_entries() {
    let alpha = Identifier::new_static("minecraft", "alpha");
    let empty = Identifier::new_static("minecraft", "empty");
    let zeta = Identifier::new_static("minecraft", "zeta");

    let mut starts = FxHashMap::default();
    starts.insert(
        zeta.clone(),
        StructureStart::new(
            zeta.clone(),
            ChunkPos::new(2, 0),
            vec![test_structure_piece()],
            TerrainAdjustment::None,
        ),
    );
    starts.insert(
        empty.clone(),
        StructureStart::new(
            empty,
            ChunkPos::new(1, 0),
            Vec::new(),
            TerrainAdjustment::None,
        ),
    );
    starts.insert(
        alpha.clone(),
        StructureStart::new(
            alpha.clone(),
            ChunkPos::new(0, 0),
            vec![test_structure_piece()],
            TerrainAdjustment::None,
        ),
    );

    let persistent_starts = ChunkStorage::structure_starts_to_persistent(&starts);
    assert_eq!(persistent_starts.len(), 2);
    assert_eq!(persistent_starts[0].structure, alpha);
    assert_eq!(persistent_starts[1].structure, zeta);

    let mut references = StructureReferenceMap::default();
    references.insert(
        Identifier::new_static("minecraft", "zeta"),
        [ChunkPos::new(2, 0), ChunkPos::new(1, 0)]
            .into_iter()
            .collect(),
    );
    references.insert(
        Identifier::new_static("minecraft", "alpha"),
        [ChunkPos::new(4, 0)].into_iter().collect(),
    );
    references.insert(
        Identifier::new_static("minecraft", "empty"),
        StructureReferenceSet::default(),
    );

    let persistent_references = ChunkStorage::structure_references_to_persistent(&references);
    assert_eq!(persistent_references.len(), 2);
    assert_eq!(
        persistent_references[0].structure,
        Identifier::new_static("minecraft", "alpha")
    );
    assert_eq!(
        persistent_references[1].structure,
        Identifier::new_static("minecraft", "zeta")
    );
    assert_eq!(
        persistent_references[1].references,
        vec![
            PackedChunkPos::from(ChunkPos::new(2, 0)),
            PackedChunkPos::from(ChunkPos::new(1, 0))
        ]
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "single fixture verifies every persisted jigsaw field roundtrips together"
)]
fn structure_start_roundtrip_preserves_typed_jigsaw_state() {
    init_vanilla_registry();

    let structure_id = Identifier::from_steel("test_jigsaw_structure");
    let piece_type = Identifier::new_static("minecraft", "jigsaw");
    let template_id = Identifier::new_static("minecraft", "village/plains/houses/test_house");
    let processor_id = Identifier::new_static("minecraft", "street_plains");

    let piece = StructurePiece {
        piece_type: piece_type.clone(),
        bounding_box: BoundingBox::new(IVec3::new(10, 64, 20), IVec3::new(15, 70, 25)),
        gen_depth: 3,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Jigsaw(JigsawPieceData {
            pool_element: PoolElement::List {
                elements: vec![
                    PoolElement::LegacySingle {
                        location: template_id.clone(),
                        processors: ProcessorList::Registry(processor_id.clone()),
                        projection: Projection::Rigid,
                    },
                    PoolElement::Feature {
                        feature: Identifier::new_static("minecraft", "pile_hay"),
                        projection: Projection::TerrainMatching,
                    },
                ],
                projection: Projection::Rigid,
            },
            position: IVec3::new(10, 64, 20),
            rotation: Rotation::Clockwise90,
            liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
        }),
        ground_level_delta: 1,
        junctions: vec![JigsawJunction {
            source_pos: IVec3::new(12, 65, 24),
            delta_y: -1,
            dest_projection: Projection::TerrainMatching,
        }],
        projection: Some(Projection::Rigid),
    };
    let start = StructureStart::new(
        structure_id.clone(),
        ChunkPos::new(4, -2),
        vec![piece],
        TerrainAdjustment::None,
    );
    let mut starts = FxHashMap::default();
    starts.insert(structure_id.clone(), start);

    let persistent = ChunkStorage::structure_starts_to_persistent(&starts);
    let encoded = wincode::serialize(&persistent).expect("structure starts should serialize");
    let decoded: Vec<PersistentStructureStart> =
        wincode::deserialize(&encoded).expect("structure starts should deserialize");
    let loaded = ChunkStorage::persistent_to_structure_starts(&decoded);

    let loaded_start = loaded
        .get(&structure_id)
        .expect("structure start should roundtrip");
    assert_eq!(loaded_start.chunk_pos, ChunkPos::new(4, -2));
    assert_eq!(loaded_start.pieces.len(), 1);

    let loaded_piece = &loaded_start.pieces[0];
    assert_eq!(loaded_piece.piece_type, piece_type);
    assert_eq!(loaded_piece.gen_depth, 3);
    assert_eq!(loaded_piece.orientation, Some(Direction::North));
    assert_eq!(loaded_piece.ground_level_delta, 1);
    assert_eq!(loaded_piece.projection, Some(Projection::Rigid));
    assert_eq!(loaded_piece.junctions.len(), 1);
    assert_eq!(
        loaded_piece.junctions[0].dest_projection,
        Projection::TerrainMatching
    );

    let StructurePiecePayload::Jigsaw(jigsaw) = &loaded_piece.payload else {
        panic!("typed jigsaw state should roundtrip");
    };
    assert_eq!(jigsaw.position, IVec3::new(10, 64, 20));
    assert_eq!(jigsaw.rotation, Rotation::Clockwise90);
    assert_eq!(
        jigsaw.liquid_settings,
        LiquidSettingsData::IgnoreWaterlogging
    );

    let PoolElement::List {
        elements,
        projection,
    } = &jigsaw.pool_element
    else {
        panic!("expected list pool element");
    };
    assert_eq!(*projection, Projection::Rigid);
    assert_eq!(elements.len(), 2);

    let PoolElement::LegacySingle {
        location,
        processors,
        projection,
    } = &elements[0]
    else {
        panic!("expected legacy single pool element");
    };
    assert_eq!(location, &template_id);
    assert_eq!(processors, &ProcessorList::Registry(processor_id));
    assert_eq!(*projection, Projection::Rigid);

    let PoolElement::Feature {
        feature,
        projection,
    } = &elements[1]
    else {
        panic!("expected feature pool element");
    };
    assert_eq!(feature, &Identifier::new_static("minecraft", "pile_hay"));
    assert_eq!(*projection, Projection::TerrainMatching);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "single roundtrip fixture covers every structure piece payload variant together"
)]
fn structure_start_roundtrip_preserves_template_and_procedural_payloads() {
    init_vanilla_registry();

    let structure_id = Identifier::from_steel("test_payload_variants");
    let template_id = Identifier::new_static("minecraft", "shipwreck/with_mast");
    let igloo_template_id = Identifier::new_static("minecraft", "igloo/top");
    let ocean_ruin_template_id = Identifier::new_static("minecraft", "underwater_ruin/warm_1");
    let processor_id = Identifier::new_static("minecraft", "zombie_plains");

    let template_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "shipwreck"),
        bounding_box: BoundingBox::new(IVec3::new(0, 70, 0), IVec3::new(12, 80, 12)),
        gen_depth: 2,
        orientation: Some(Direction::East),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id: template_id.clone(),
            template_position: IVec3::new(1, 70, 2),
            rotation: Rotation::Clockwise180,
            mirror: StructureMirror::FrontBack,
            rotation_pivot: IVec3::new(4, 0, 15),
            block_ignore: StructureBlockIgnore::StructureAndAir,
            late_block_ignore: StructureBlockIgnore::None,
            processors: TemplateProcessorList::Registry(processor_id.clone()),
            liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
            marker_handling: TemplateMarkerHandling::DataMarkers,
            placement_adjustment: TemplatePlacementAdjustment::Shipwreck {
                is_beached: true,
                height_adjusted: false,
            },
            placement_clip: TemplatePlacementClip::CenterChunkExpandedToTemplate,
            post_process: TemplatePostProcess::NetherFossil,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let igloo_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "iglu"),
        bounding_box: BoundingBox::new(IVec3::new(4, 80, 4), IVec3::new(10, 84, 11)),
        gen_depth: 0,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id: igloo_template_id.clone(),
            template_position: IVec3::new(4, 90, 4),
            rotation: Rotation::Clockwise90,
            mirror: StructureMirror::None,
            rotation_pivot: IVec3::new(3, 5, 5),
            block_ignore: StructureBlockIgnore::StructureBlock,
            late_block_ignore: StructureBlockIgnore::None,
            processors: TemplateProcessorList::Empty,
            liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
            marker_handling: TemplateMarkerHandling::Igloo,
            placement_adjustment: TemplatePlacementAdjustment::Igloo {
                template_offset: (0, 0, 0),
            },
            placement_clip: TemplatePlacementClip::CenterChunk,
            post_process: TemplatePostProcess::IglooTop,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let ocean_ruin_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "orp"),
        bounding_box: BoundingBox::new(IVec3::new(12, 90, 12), IVec3::new(20, 96, 20)),
        gen_depth: 0,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Template(TemplatePieceData {
            template_id: ocean_ruin_template_id.clone(),
            template_position: IVec3::new(12, 90, 12),
            rotation: Rotation::CounterClockwise90,
            mirror: StructureMirror::None,
            rotation_pivot: IVec3::new(0, 0, 0),
            block_ignore: StructureBlockIgnore::None,
            late_block_ignore: StructureBlockIgnore::StructureAndAir,
            processors: TemplateProcessorList::OceanRuin {
                biome_temp: OceanRuinBiomeTempData::Warm,
                integrity: 0.8,
            },
            liquid_settings: LiquidSettingsData::ApplyWaterlogging,
            marker_handling: TemplateMarkerHandling::OceanRuin { is_large: false },
            placement_adjustment: TemplatePlacementAdjustment::OceanRuin,
            placement_clip: TemplatePlacementClip::CenterChunk,
            post_process: TemplatePostProcess::None,
        }),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let procedural_piece = StructurePiece::non_jigsaw(
        Identifier::new_static("minecraft", "mscorridor"),
        BoundingBox::new(IVec3::new(20, 40, 20), IVec3::new(30, 50, 30)),
        5,
        Some(Direction::South),
    );
    let buried_treasure_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "btp"),
        bounding_box: BoundingBox::new(IVec3::new(41, 90, 43), IVec3::new(41, 90, 43)),
        gen_depth: 0,
        orientation: None,
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::BuriedTreasure),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let desert_pyramid_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "tedp"),
        bounding_box: BoundingBox::new(IVec3::new(48, 63, 48), IVec3::new(68, 77, 68)),
        gen_depth: 0,
        orientation: Some(Direction::East),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::DesertPyramid(
            DesertPyramidPieceData {
                height_position: Some(63),
                has_placed_chest: [true, false, true, false],
                potential_suspicious_sand_world_positions: vec![BlockPos::new(51, 64, 54)],
                random_collapsed_roof_pos: BlockPos::new(50, 64, 50),
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let jungle_temple_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "tejp"),
        bounding_box: BoundingBox::new(IVec3::new(64, 63, 64), IVec3::new(75, 72, 78)),
        gen_depth: 0,
        orientation: Some(Direction::South),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::JungleTemple(
            JungleTemplePieceData {
                height_position: Some(64),
                placed_main_chest: true,
                placed_hidden_chest: false,
                placed_trap1: true,
                placed_trap2: false,
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let mineshaft_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "mscorridor"),
        bounding_box: BoundingBox::new(IVec3::new(32, 45, 32), IVec3::new(34, 47, 46)),
        gen_depth: 4,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(
            MineshaftPiecePayload {
                mineshaft_type: MineshaftType::Mesa,
                kind: MineshaftPieceKind::Corridor {
                    has_rails: true,
                    spider_corridor: false,
                    has_placed_spider: true,
                    num_sections: 3,
                },
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let fortress_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "nemt"),
        bounding_box: BoundingBox::new(IVec3::new(48, 52, 48), IVec3::new(54, 59, 56)),
        gen_depth: 6,
        orientation: Some(Direction::East),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::NetherFortress(
            FortressPieceData::MonsterThrone {
                has_placed_spawner: true,
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let ocean_monument_room = OceanMonumentRoomData {
        index: 12,
        has_opening: [false, true, true, false, true, false],
        has_up_connection: true,
    };
    let ocean_monument_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "omb"),
        bounding_box: BoundingBox::new(IVec3::new(64, 39, 64), IVec3::new(121, 61, 121)),
        gen_depth: 0,
        orientation: Some(Direction::South),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::OceanMonument(
            OceanMonumentPieceData {
                child_pieces: vec![
                    OceanMonumentChildPiece {
                        bounding_box: BoundingBox::new(
                            IVec3::new(73, 39, 86),
                            IVec3::new(80, 42, 93),
                        ),
                        kind: OceanMonumentChildPieceKind::SimpleRoom {
                            room: ocean_monument_room,
                            main_design: 2,
                        },
                    },
                    OceanMonumentChildPiece {
                        bounding_box: BoundingBox::new(
                            IVec3::new(65, 40, 65),
                            IVec3::new(87, 47, 85),
                        ),
                        kind: OceanMonumentChildPieceKind::WingRoom { main_design: 1 },
                    },
                    OceanMonumentChildPiece {
                        bounding_box: BoundingBox::new(
                            IVec3::new(86, 52, 86),
                            IVec3::new(99, 56, 99),
                        ),
                        kind: OceanMonumentChildPieceKind::Penthouse,
                    },
                ],
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let stronghold_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "shrc"),
        bounding_box: BoundingBox::new(IVec3::new(55, 35, 55), IVec3::new(65, 41, 65)),
        gen_depth: 7,
        orientation: Some(Direction::North),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::Stronghold(
            StrongholdPieceData::RoomCrossing {
                entry_door: StrongholdSmallDoorType::IronDoor,
                crossing_type: 2,
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };
    let swamp_hut_piece = StructurePiece {
        piece_type: Identifier::new_static("minecraft", "tesh"),
        bounding_box: BoundingBox::new(IVec3::new(80, 63, 80), IVec3::new(86, 69, 88)),
        gen_depth: 0,
        orientation: Some(Direction::West),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::SwampHut(
            SwampHutPieceData {
                height_position: Some(62),
                spawned_witch: true,
                spawned_cat: false,
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    };

    let start = StructureStart::new(
        structure_id.clone(),
        ChunkPos::new(8, 9),
        vec![
            template_piece,
            igloo_piece,
            ocean_ruin_piece,
            procedural_piece,
            buried_treasure_piece,
            desert_pyramid_piece,
            jungle_temple_piece,
            mineshaft_piece,
            fortress_piece,
            ocean_monument_piece,
            stronghold_piece,
            swamp_hut_piece,
        ],
        TerrainAdjustment::None,
    );
    let mut starts = FxHashMap::default();
    starts.insert(structure_id.clone(), start);

    let persistent = ChunkStorage::structure_starts_to_persistent(&starts);
    let encoded = wincode::serialize(&persistent).expect("structure starts should serialize");
    let decoded: Vec<PersistentStructureStart> =
        wincode::deserialize(&encoded).expect("structure starts should deserialize");
    let loaded = ChunkStorage::persistent_to_structure_starts(&decoded);
    let loaded_start = loaded
        .get(&structure_id)
        .expect("structure start should roundtrip");
    assert_eq!(loaded_start.pieces.len(), 12);

    let StructurePiecePayload::Template(template) = &loaded_start.pieces[0].payload else {
        panic!("template payload should roundtrip");
    };
    assert_eq!(template.template_id, template_id);
    assert_eq!(template.template_position, IVec3::new(1, 70, 2));
    assert_eq!(template.rotation, Rotation::Clockwise180);
    assert_eq!(template.mirror, StructureMirror::FrontBack);
    assert_eq!(template.rotation_pivot, IVec3::new(4, 0, 15));
    assert_eq!(template.block_ignore, StructureBlockIgnore::StructureAndAir);
    assert_eq!(template.late_block_ignore, StructureBlockIgnore::None);
    assert_eq!(
        template.liquid_settings,
        LiquidSettingsData::IgnoreWaterlogging
    );
    assert_eq!(
        template.marker_handling,
        TemplateMarkerHandling::DataMarkers
    );
    assert_eq!(
        template.placement_adjustment,
        TemplatePlacementAdjustment::Shipwreck {
            is_beached: true,
            height_adjusted: false,
        }
    );
    assert_eq!(
        template.placement_clip,
        TemplatePlacementClip::CenterChunkExpandedToTemplate
    );
    assert_eq!(template.post_process, TemplatePostProcess::NetherFossil);
    assert_eq!(
        template.processors,
        TemplateProcessorList::Registry(processor_id.clone())
    );

    let StructurePiecePayload::Template(template) = &loaded_start.pieces[1].payload else {
        panic!("igloo template payload should roundtrip");
    };
    assert_eq!(template.template_id, igloo_template_id);
    assert_eq!(template.template_position, IVec3::new(4, 90, 4));
    assert_eq!(template.rotation, Rotation::Clockwise90);
    assert_eq!(template.mirror, StructureMirror::None);
    assert_eq!(template.rotation_pivot, IVec3::new(3, 5, 5));
    assert_eq!(template.block_ignore, StructureBlockIgnore::StructureBlock);
    assert_eq!(template.late_block_ignore, StructureBlockIgnore::None);
    assert_eq!(template.processors, TemplateProcessorList::Empty);
    assert_eq!(template.marker_handling, TemplateMarkerHandling::Igloo);
    assert_eq!(
        template.placement_adjustment,
        TemplatePlacementAdjustment::Igloo {
            template_offset: (0, 0, 0),
        }
    );
    assert_eq!(template.placement_clip, TemplatePlacementClip::CenterChunk);
    assert_eq!(template.post_process, TemplatePostProcess::IglooTop);

    let StructurePiecePayload::Template(template) = &loaded_start.pieces[2].payload else {
        panic!("ocean ruin template payload should roundtrip");
    };
    assert_eq!(template.template_id, ocean_ruin_template_id);
    assert_eq!(template.template_position, IVec3::new(12, 90, 12));
    assert_eq!(template.rotation, Rotation::CounterClockwise90);
    assert_eq!(template.mirror, StructureMirror::None);
    assert_eq!(template.rotation_pivot, IVec3::new(0, 0, 0));
    assert_eq!(template.block_ignore, StructureBlockIgnore::None);
    assert_eq!(
        template.late_block_ignore,
        StructureBlockIgnore::StructureAndAir
    );
    assert_eq!(
        template.processors,
        TemplateProcessorList::OceanRuin {
            biome_temp: OceanRuinBiomeTempData::Warm,
            integrity: 0.8,
        }
    );
    assert_eq!(
        template.marker_handling,
        TemplateMarkerHandling::OceanRuin { is_large: false }
    );
    assert_eq!(
        template.placement_adjustment,
        TemplatePlacementAdjustment::OceanRuin
    );
    assert_eq!(template.placement_clip, TemplatePlacementClip::CenterChunk);
    assert_eq!(template.post_process, TemplatePostProcess::None);

    assert!(matches!(
        loaded_start.pieces[3].payload,
        StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented)
    ));
    assert!(matches!(
        loaded_start.pieces[4].payload,
        StructurePiecePayload::Procedural(ProceduralPieceData::BuriedTreasure)
    ));

    let StructurePiecePayload::Procedural(ProceduralPieceData::DesertPyramid(payload)) =
        &loaded_start.pieces[5].payload
    else {
        panic!("desert pyramid payload should roundtrip");
    };
    assert_eq!(payload.height_position, Some(63));
    assert_eq!(payload.has_placed_chest, [true, false, true, false]);
    assert!(payload.potential_suspicious_sand_world_positions.is_empty());
    assert_eq!(payload.random_collapsed_roof_pos, BlockPos::new(0, 0, 0));

    let StructurePiecePayload::Procedural(ProceduralPieceData::JungleTemple(payload)) =
        &loaded_start.pieces[6].payload
    else {
        panic!("jungle temple payload should roundtrip");
    };
    assert_eq!(payload.height_position, Some(64));
    assert!(payload.placed_main_chest);
    assert!(!payload.placed_hidden_chest);
    assert!(payload.placed_trap1);
    assert!(!payload.placed_trap2);

    let StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(payload)) =
        &loaded_start.pieces[7].payload
    else {
        panic!("mineshaft payload should roundtrip");
    };
    assert_eq!(payload.mineshaft_type, MineshaftType::Mesa);
    let MineshaftPieceKind::Corridor {
        has_rails,
        spider_corridor,
        has_placed_spider,
        num_sections,
    } = &payload.kind
    else {
        panic!("expected mineshaft corridor payload");
    };
    assert!(*has_rails);
    assert!(!*spider_corridor);
    assert!(*has_placed_spider);
    assert_eq!(*num_sections, 3);

    let StructurePiecePayload::Procedural(ProceduralPieceData::NetherFortress(fortress_payload)) =
        &loaded_start.pieces[8].payload
    else {
        panic!("nether fortress payload should roundtrip");
    };
    assert_eq!(
        *fortress_payload,
        FortressPieceData::MonsterThrone {
            has_placed_spawner: true,
        }
    );

    let StructurePiecePayload::Procedural(ProceduralPieceData::OceanMonument(payload)) =
        &loaded_start.pieces[9].payload
    else {
        panic!("ocean monument payload should roundtrip");
    };
    assert_eq!(payload.child_pieces.len(), 3);
    let OceanMonumentChildPieceKind::SimpleRoom { room, main_design } =
        &payload.child_pieces[0].kind
    else {
        panic!("ocean monument simple room child should roundtrip");
    };
    assert_eq!(*room, ocean_monument_room);
    assert_eq!(*main_design, 2);
    assert!(matches!(
        payload.child_pieces[1].kind,
        OceanMonumentChildPieceKind::WingRoom { main_design: 1 }
    ));
    assert!(matches!(
        payload.child_pieces[2].kind,
        OceanMonumentChildPieceKind::Penthouse
    ));

    let StructurePiecePayload::Procedural(ProceduralPieceData::Stronghold(stronghold_payload)) =
        &loaded_start.pieces[10].payload
    else {
        panic!("stronghold payload should roundtrip");
    };
    assert_eq!(
        *stronghold_payload,
        StrongholdPieceData::RoomCrossing {
            entry_door: StrongholdSmallDoorType::IronDoor,
            crossing_type: 2,
        }
    );

    let StructurePiecePayload::Procedural(ProceduralPieceData::SwampHut(payload)) =
        &loaded_start.pieces[11].payload
    else {
        panic!("swamp hut payload should roundtrip");
    };
    assert_eq!(payload.height_position, Some(62));
    assert!(payload.spawned_witch);
    assert!(!payload.spawned_cat);
}

#[test]
fn template_processor_list_roundtrips_ruined_portal_processors() {
    let ocean_ruin_processors = TemplateProcessorList::OceanRuin {
        biome_temp: OceanRuinBiomeTempData::Cold,
        integrity: 0.7,
    };
    let persistent = ChunkStorage::template_processors_to_persistent(&ocean_ruin_processors);
    let loaded = ChunkStorage::persistent_to_template_processors(&persistent);
    assert_eq!(loaded, ocean_ruin_processors);

    let processors = TemplateProcessorList::RuinedPortal {
        vertical_placement: RuinedPortalPlacementData::OnOceanFloor,
        properties: RuinedPortalProperties {
            cold: true,
            mossiness: 0.8,
            air_pocket: false,
            overgrown: true,
            vines: true,
            replace_with_blackstone: false,
        },
    };

    let persistent = ChunkStorage::template_processors_to_persistent(&processors);
    let loaded = ChunkStorage::persistent_to_template_processors(&persistent);

    assert_eq!(loaded, processors);
    assert_eq!(
        placement_clip_from_persistent(placement_clip_to_persistent(
            TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
        )),
        TemplatePlacementClip::CenterChunkContainsTemplateCenterExpandedToTemplate,
    );
    assert_eq!(
        post_process_from_persistent(post_process_to_persistent(
            TemplatePostProcess::RuinedPortal
        )),
        TemplatePostProcess::RuinedPortal,
    );
    assert_eq!(
        marker_handling_from_persistent(marker_handling_to_persistent(
            TemplateMarkerHandling::EndCity
        )),
        TemplateMarkerHandling::EndCity,
    );
    assert_eq!(
        marker_handling_from_persistent(marker_handling_to_persistent(
            TemplateMarkerHandling::WoodlandMansion
        )),
        TemplateMarkerHandling::WoodlandMansion,
    );
}
