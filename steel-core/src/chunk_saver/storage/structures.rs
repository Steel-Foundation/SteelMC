use super::{
    BlockPos, ChunkPos, ChunkStorage, DesertPyramidPieceData, FortressPieceData, IVec3,
    JigsawJunction, JigsawPieceData, JungleTemplePieceData, MineshaftPieceKind,
    MineshaftPiecePayload, OceanMonumentChildPiece, OceanMonumentChildPieceKind,
    OceanMonumentPieceData, OceanMonumentRoomData, PackedChunkPos, PersistentBoundingBox,
    PersistentDesertPyramidPieceData, PersistentJigsawJunction, PersistentJigsawPieceData,
    PersistentJungleTemplePieceData, PersistentMineshaftPieceData, PersistentMineshaftPieceKind,
    PersistentNetherFortressPieceData, PersistentOceanMonumentChildPiece,
    PersistentOceanMonumentChildPieceKind, PersistentOceanMonumentPieceData,
    PersistentOceanMonumentRoomData, PersistentPoolElement, PersistentProceduralPieceData,
    PersistentProcessorList, PersistentStrongholdPieceData, PersistentStrongholdSmallDoorType,
    PersistentStructurePiece, PersistentStructurePiecePayload, PersistentStructureReference,
    PersistentStructureStart, PersistentSwampHutPieceData, PersistentTemplatePieceData,
    PersistentTemplateProcessorList, PoolElement, ProceduralPieceData, ProcessorList, REGISTRY,
    RegistryExt, RuinedPortalProperties, StrongholdPieceData, StrongholdSmallDoorType,
    StructurePiece, StructurePiecePayload, StructureReferenceMap, StructureStart,
    StructureStartMap, SwampHutPieceData, TemplatePieceData, TemplateProcessorList,
    TerrainAdjustment, block_ignore_from_persistent, block_ignore_to_persistent,
    compare_identifiers, direction_from_2d, direction_to_2d, liquid_settings_from_persistent,
    liquid_settings_to_persistent, marker_handling_from_persistent, marker_handling_to_persistent,
    mineshaft_type_from_persistent, mineshaft_type_to_persistent, mirror_from_persistent,
    mirror_to_persistent, ocean_ruin_biome_temp_from_persistent,
    ocean_ruin_biome_temp_to_persistent, placement_adjustment_from_persistent,
    placement_adjustment_to_persistent, placement_clip_from_persistent,
    placement_clip_to_persistent, post_process_from_persistent, post_process_to_persistent,
    projection_from_persistent, projection_to_persistent, required_direction_from_2d,
    required_projection_from_persistent, rotation_from_persistent, rotation_to_persistent,
    ruined_portal_placement_from_persistent, ruined_portal_placement_to_persistent,
};

impl ChunkStorage {
    pub(super) fn jigsaw_piece_data_to_persistent(
        data: &JigsawPieceData,
    ) -> PersistentJigsawPieceData {
        PersistentJigsawPieceData {
            pool_element: Self::pool_element_to_persistent(&data.pool_element),
            position: [data.position.x, data.position.y, data.position.z],
            rotation: rotation_to_persistent(data.rotation),
            liquid_settings: liquid_settings_to_persistent(data.liquid_settings),
        }
    }

    pub(super) fn persistent_to_jigsaw_piece_data(
        data: &PersistentJigsawPieceData,
    ) -> JigsawPieceData {
        JigsawPieceData {
            pool_element: Self::persistent_to_pool_element(&data.pool_element),
            position: IVec3::new(data.position[0], data.position[1], data.position[2]),
            rotation: rotation_from_persistent(data.rotation),
            liquid_settings: liquid_settings_from_persistent(data.liquid_settings),
        }
    }

    pub(super) fn procedural_piece_data_to_persistent(
        data: &ProceduralPieceData,
    ) -> PersistentProceduralPieceData {
        match data {
            ProceduralPieceData::Unimplemented => PersistentProceduralPieceData::Unimplemented,
            ProceduralPieceData::BuriedTreasure => PersistentProceduralPieceData::BuriedTreasure,
            ProceduralPieceData::DesertPyramid(data) => {
                PersistentProceduralPieceData::DesertPyramid(PersistentDesertPyramidPieceData {
                    height_position: data.height_position.unwrap_or(-1),
                    has_placed_chest: data.has_placed_chest,
                })
            }
            ProceduralPieceData::JungleTemple(data) => {
                PersistentProceduralPieceData::JungleTemple(PersistentJungleTemplePieceData {
                    height_position: data.height_position.unwrap_or(-1),
                    placed_main_chest: data.placed_main_chest,
                    placed_hidden_chest: data.placed_hidden_chest,
                    placed_trap1: data.placed_trap1,
                    placed_trap2: data.placed_trap2,
                })
            }
            ProceduralPieceData::Mineshaft(data) => {
                PersistentProceduralPieceData::Mineshaft(PersistentMineshaftPieceData {
                    mineshaft_type: mineshaft_type_to_persistent(data.mineshaft_type),
                    kind: Self::mineshaft_kind_to_persistent(&data.kind),
                })
            }
            ProceduralPieceData::NetherFortress(data) => {
                PersistentProceduralPieceData::NetherFortress(
                    Self::fortress_piece_data_to_persistent(*data),
                )
            }
            ProceduralPieceData::OceanMonument(data) => {
                PersistentProceduralPieceData::OceanMonument(
                    Self::ocean_monument_data_to_persistent(data),
                )
            }
            ProceduralPieceData::Stronghold(data) => PersistentProceduralPieceData::Stronghold(
                Self::stronghold_piece_data_to_persistent(*data),
            ),
            ProceduralPieceData::SwampHut(data) => {
                PersistentProceduralPieceData::SwampHut(PersistentSwampHutPieceData {
                    height_position: data.height_position.unwrap_or(-1),
                    spawned_witch: data.spawned_witch,
                    spawned_cat: data.spawned_cat,
                })
            }
        }
    }

    pub(super) fn persistent_to_procedural_piece_data(
        data: &PersistentProceduralPieceData,
    ) -> ProceduralPieceData {
        match data {
            PersistentProceduralPieceData::Unimplemented => ProceduralPieceData::Unimplemented,
            PersistentProceduralPieceData::BuriedTreasure => ProceduralPieceData::BuriedTreasure,
            PersistentProceduralPieceData::DesertPyramid(data) => {
                ProceduralPieceData::DesertPyramid(DesertPyramidPieceData {
                    height_position: (data.height_position >= 0).then_some(data.height_position),
                    has_placed_chest: data.has_placed_chest,
                    potential_suspicious_sand_world_positions: Vec::new(),
                    random_collapsed_roof_pos: BlockPos::new(0, 0, 0),
                })
            }
            PersistentProceduralPieceData::JungleTemple(data) => {
                ProceduralPieceData::JungleTemple(JungleTemplePieceData {
                    height_position: (data.height_position >= 0).then_some(data.height_position),
                    placed_main_chest: data.placed_main_chest,
                    placed_hidden_chest: data.placed_hidden_chest,
                    placed_trap1: data.placed_trap1,
                    placed_trap2: data.placed_trap2,
                })
            }
            PersistentProceduralPieceData::Mineshaft(data) => {
                ProceduralPieceData::Mineshaft(MineshaftPiecePayload {
                    mineshaft_type: mineshaft_type_from_persistent(data.mineshaft_type),
                    kind: Self::persistent_to_mineshaft_kind(&data.kind),
                })
            }
            PersistentProceduralPieceData::NetherFortress(data) => {
                ProceduralPieceData::NetherFortress(Self::persistent_to_fortress_piece_data(data))
            }
            PersistentProceduralPieceData::OceanMonument(data) => {
                ProceduralPieceData::OceanMonument(Self::persistent_to_ocean_monument_data(data))
            }
            PersistentProceduralPieceData::Stronghold(data) => {
                ProceduralPieceData::Stronghold(Self::persistent_to_stronghold_piece_data(data))
            }
            PersistentProceduralPieceData::SwampHut(data) => {
                ProceduralPieceData::SwampHut(SwampHutPieceData {
                    height_position: (data.height_position >= 0).then_some(data.height_position),
                    spawned_witch: data.spawned_witch,
                    spawned_cat: data.spawned_cat,
                })
            }
        }
    }

    pub(super) fn ocean_monument_data_to_persistent(
        data: &OceanMonumentPieceData,
    ) -> PersistentOceanMonumentPieceData {
        PersistentOceanMonumentPieceData {
            child_pieces: data
                .child_pieces
                .iter()
                .map(Self::ocean_monument_child_to_persistent)
                .collect(),
        }
    }

    pub(super) fn persistent_to_ocean_monument_data(
        data: &PersistentOceanMonumentPieceData,
    ) -> OceanMonumentPieceData {
        OceanMonumentPieceData {
            child_pieces: data
                .child_pieces
                .iter()
                .map(Self::persistent_to_ocean_monument_child)
                .collect(),
        }
    }

    const fn ocean_monument_child_to_persistent(
        child: &OceanMonumentChildPiece,
    ) -> PersistentOceanMonumentChildPiece {
        PersistentOceanMonumentChildPiece {
            bounding_box: PersistentBoundingBox::from_bounding_box(child.bounding_box),
            kind: Self::ocean_monument_child_kind_to_persistent(&child.kind),
        }
    }

    const fn persistent_to_ocean_monument_child(
        child: &PersistentOceanMonumentChildPiece,
    ) -> OceanMonumentChildPiece {
        OceanMonumentChildPiece {
            bounding_box: child.bounding_box.to_bounding_box(),
            kind: Self::persistent_to_ocean_monument_child_kind(&child.kind),
        }
    }

    const fn ocean_monument_child_kind_to_persistent(
        kind: &OceanMonumentChildPieceKind,
    ) -> PersistentOceanMonumentChildPieceKind {
        match kind {
            OceanMonumentChildPieceKind::EntryRoom { room } => {
                PersistentOceanMonumentChildPieceKind::EntryRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                }
            }
            OceanMonumentChildPieceKind::CoreRoom => {
                PersistentOceanMonumentChildPieceKind::CoreRoom
            }
            OceanMonumentChildPieceKind::DoubleXRoom { west, east } => {
                PersistentOceanMonumentChildPieceKind::DoubleXRoom {
                    west: Self::ocean_monument_room_to_persistent(*west),
                    east: Self::ocean_monument_room_to_persistent(*east),
                }
            }
            OceanMonumentChildPieceKind::DoubleXYRoom {
                west,
                east,
                west_up,
                east_up,
            } => PersistentOceanMonumentChildPieceKind::DoubleXYRoom {
                west: Self::ocean_monument_room_to_persistent(*west),
                east: Self::ocean_monument_room_to_persistent(*east),
                west_up: Self::ocean_monument_room_to_persistent(*west_up),
                east_up: Self::ocean_monument_room_to_persistent(*east_up),
            },
            OceanMonumentChildPieceKind::DoubleYRoom { room, above } => {
                PersistentOceanMonumentChildPieceKind::DoubleYRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                    above: Self::ocean_monument_room_to_persistent(*above),
                }
            }
            OceanMonumentChildPieceKind::DoubleYZRoom {
                south,
                north,
                south_up,
                north_up,
            } => PersistentOceanMonumentChildPieceKind::DoubleYZRoom {
                south: Self::ocean_monument_room_to_persistent(*south),
                north: Self::ocean_monument_room_to_persistent(*north),
                south_up: Self::ocean_monument_room_to_persistent(*south_up),
                north_up: Self::ocean_monument_room_to_persistent(*north_up),
            },
            OceanMonumentChildPieceKind::DoubleZRoom { south, north } => {
                PersistentOceanMonumentChildPieceKind::DoubleZRoom {
                    south: Self::ocean_monument_room_to_persistent(*south),
                    north: Self::ocean_monument_room_to_persistent(*north),
                }
            }
            OceanMonumentChildPieceKind::SimpleRoom { room, main_design } => {
                PersistentOceanMonumentChildPieceKind::SimpleRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                    main_design: *main_design,
                }
            }
            OceanMonumentChildPieceKind::SimpleTopRoom { room } => {
                PersistentOceanMonumentChildPieceKind::SimpleTopRoom {
                    room: Self::ocean_monument_room_to_persistent(*room),
                }
            }
            OceanMonumentChildPieceKind::WingRoom { main_design } => {
                PersistentOceanMonumentChildPieceKind::WingRoom {
                    main_design: *main_design,
                }
            }
            OceanMonumentChildPieceKind::Penthouse => {
                PersistentOceanMonumentChildPieceKind::Penthouse
            }
        }
    }

    const fn persistent_to_ocean_monument_child_kind(
        kind: &PersistentOceanMonumentChildPieceKind,
    ) -> OceanMonumentChildPieceKind {
        match kind {
            PersistentOceanMonumentChildPieceKind::EntryRoom { room } => {
                OceanMonumentChildPieceKind::EntryRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                }
            }
            PersistentOceanMonumentChildPieceKind::CoreRoom => {
                OceanMonumentChildPieceKind::CoreRoom
            }
            PersistentOceanMonumentChildPieceKind::DoubleXRoom { west, east } => {
                OceanMonumentChildPieceKind::DoubleXRoom {
                    west: Self::persistent_to_ocean_monument_room(west),
                    east: Self::persistent_to_ocean_monument_room(east),
                }
            }
            PersistentOceanMonumentChildPieceKind::DoubleXYRoom {
                west,
                east,
                west_up,
                east_up,
            } => OceanMonumentChildPieceKind::DoubleXYRoom {
                west: Self::persistent_to_ocean_monument_room(west),
                east: Self::persistent_to_ocean_monument_room(east),
                west_up: Self::persistent_to_ocean_monument_room(west_up),
                east_up: Self::persistent_to_ocean_monument_room(east_up),
            },
            PersistentOceanMonumentChildPieceKind::DoubleYRoom { room, above } => {
                OceanMonumentChildPieceKind::DoubleYRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                    above: Self::persistent_to_ocean_monument_room(above),
                }
            }
            PersistentOceanMonumentChildPieceKind::DoubleYZRoom {
                south,
                north,
                south_up,
                north_up,
            } => OceanMonumentChildPieceKind::DoubleYZRoom {
                south: Self::persistent_to_ocean_monument_room(south),
                north: Self::persistent_to_ocean_monument_room(north),
                south_up: Self::persistent_to_ocean_monument_room(south_up),
                north_up: Self::persistent_to_ocean_monument_room(north_up),
            },
            PersistentOceanMonumentChildPieceKind::DoubleZRoom { south, north } => {
                OceanMonumentChildPieceKind::DoubleZRoom {
                    south: Self::persistent_to_ocean_monument_room(south),
                    north: Self::persistent_to_ocean_monument_room(north),
                }
            }
            PersistentOceanMonumentChildPieceKind::SimpleRoom { room, main_design } => {
                OceanMonumentChildPieceKind::SimpleRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                    main_design: *main_design,
                }
            }
            PersistentOceanMonumentChildPieceKind::SimpleTopRoom { room } => {
                OceanMonumentChildPieceKind::SimpleTopRoom {
                    room: Self::persistent_to_ocean_monument_room(room),
                }
            }
            PersistentOceanMonumentChildPieceKind::WingRoom { main_design } => {
                OceanMonumentChildPieceKind::WingRoom {
                    main_design: *main_design,
                }
            }
            PersistentOceanMonumentChildPieceKind::Penthouse => {
                OceanMonumentChildPieceKind::Penthouse
            }
        }
    }

    const fn ocean_monument_room_to_persistent(
        room: OceanMonumentRoomData,
    ) -> PersistentOceanMonumentRoomData {
        PersistentOceanMonumentRoomData {
            index: room.index,
            has_opening: room.has_opening,
            has_up_connection: room.has_up_connection,
        }
    }

    const fn persistent_to_ocean_monument_room(
        room: &PersistentOceanMonumentRoomData,
    ) -> OceanMonumentRoomData {
        OceanMonumentRoomData {
            index: room.index,
            has_opening: room.has_opening,
            has_up_connection: room.has_up_connection,
        }
    }

    const fn fortress_piece_data_to_persistent(
        data: FortressPieceData,
    ) -> PersistentNetherFortressPieceData {
        match data {
            FortressPieceData::BridgeCrossing => PersistentNetherFortressPieceData::BridgeCrossing,
            FortressPieceData::BridgeEndFiller { self_seed } => {
                PersistentNetherFortressPieceData::BridgeEndFiller { self_seed }
            }
            FortressPieceData::BridgeStraight => PersistentNetherFortressPieceData::BridgeStraight,
            FortressPieceData::CastleCorridorStairs => {
                PersistentNetherFortressPieceData::CastleCorridorStairs
            }
            FortressPieceData::CastleCorridorTBalcony => {
                PersistentNetherFortressPieceData::CastleCorridorTBalcony
            }
            FortressPieceData::CastleEntrance => PersistentNetherFortressPieceData::CastleEntrance,
            FortressPieceData::CastleSmallCorridorCrossing => {
                PersistentNetherFortressPieceData::CastleSmallCorridorCrossing
            }
            FortressPieceData::CastleSmallCorridorLeftTurn { is_needing_chest } => {
                PersistentNetherFortressPieceData::CastleSmallCorridorLeftTurn { is_needing_chest }
            }
            FortressPieceData::CastleSmallCorridor => {
                PersistentNetherFortressPieceData::CastleSmallCorridor
            }
            FortressPieceData::CastleSmallCorridorRightTurn { is_needing_chest } => {
                PersistentNetherFortressPieceData::CastleSmallCorridorRightTurn { is_needing_chest }
            }
            FortressPieceData::CastleStalkRoom => {
                PersistentNetherFortressPieceData::CastleStalkRoom
            }
            FortressPieceData::MonsterThrone { has_placed_spawner } => {
                PersistentNetherFortressPieceData::MonsterThrone { has_placed_spawner }
            }
            FortressPieceData::RoomCrossing => PersistentNetherFortressPieceData::RoomCrossing,
            FortressPieceData::StairsRoom => PersistentNetherFortressPieceData::StairsRoom,
        }
    }

    const fn persistent_to_fortress_piece_data(
        data: &PersistentNetherFortressPieceData,
    ) -> FortressPieceData {
        match data {
            PersistentNetherFortressPieceData::BridgeCrossing => FortressPieceData::BridgeCrossing,
            PersistentNetherFortressPieceData::BridgeEndFiller { self_seed } => {
                FortressPieceData::BridgeEndFiller {
                    self_seed: *self_seed,
                }
            }
            PersistentNetherFortressPieceData::BridgeStraight => FortressPieceData::BridgeStraight,
            PersistentNetherFortressPieceData::CastleCorridorStairs => {
                FortressPieceData::CastleCorridorStairs
            }
            PersistentNetherFortressPieceData::CastleCorridorTBalcony => {
                FortressPieceData::CastleCorridorTBalcony
            }
            PersistentNetherFortressPieceData::CastleEntrance => FortressPieceData::CastleEntrance,
            PersistentNetherFortressPieceData::CastleSmallCorridorCrossing => {
                FortressPieceData::CastleSmallCorridorCrossing
            }
            PersistentNetherFortressPieceData::CastleSmallCorridorLeftTurn { is_needing_chest } => {
                FortressPieceData::CastleSmallCorridorLeftTurn {
                    is_needing_chest: *is_needing_chest,
                }
            }
            PersistentNetherFortressPieceData::CastleSmallCorridor => {
                FortressPieceData::CastleSmallCorridor
            }
            PersistentNetherFortressPieceData::CastleSmallCorridorRightTurn {
                is_needing_chest,
            } => FortressPieceData::CastleSmallCorridorRightTurn {
                is_needing_chest: *is_needing_chest,
            },
            PersistentNetherFortressPieceData::CastleStalkRoom => {
                FortressPieceData::CastleStalkRoom
            }
            PersistentNetherFortressPieceData::MonsterThrone { has_placed_spawner } => {
                FortressPieceData::MonsterThrone {
                    has_placed_spawner: *has_placed_spawner,
                }
            }
            PersistentNetherFortressPieceData::RoomCrossing => FortressPieceData::RoomCrossing,
            PersistentNetherFortressPieceData::StairsRoom => FortressPieceData::StairsRoom,
        }
    }

    const fn stronghold_door_to_persistent(
        door: StrongholdSmallDoorType,
    ) -> PersistentStrongholdSmallDoorType {
        match door {
            StrongholdSmallDoorType::Opening => PersistentStrongholdSmallDoorType::Opening,
            StrongholdSmallDoorType::WoodDoor => PersistentStrongholdSmallDoorType::WoodDoor,
            StrongholdSmallDoorType::Grates => PersistentStrongholdSmallDoorType::Grates,
            StrongholdSmallDoorType::IronDoor => PersistentStrongholdSmallDoorType::IronDoor,
        }
    }

    const fn persistent_to_stronghold_door(
        door: &PersistentStrongholdSmallDoorType,
    ) -> StrongholdSmallDoorType {
        match door {
            PersistentStrongholdSmallDoorType::Opening => StrongholdSmallDoorType::Opening,
            PersistentStrongholdSmallDoorType::WoodDoor => StrongholdSmallDoorType::WoodDoor,
            PersistentStrongholdSmallDoorType::Grates => StrongholdSmallDoorType::Grates,
            PersistentStrongholdSmallDoorType::IronDoor => StrongholdSmallDoorType::IronDoor,
        }
    }

    const fn stronghold_piece_data_to_persistent(
        data: StrongholdPieceData,
    ) -> PersistentStrongholdPieceData {
        match data {
            StrongholdPieceData::Straight {
                entry_door,
                left_child,
                right_child,
            } => PersistentStrongholdPieceData::Straight {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                left_child,
                right_child,
            },
            StrongholdPieceData::PrisonHall { entry_door } => {
                PersistentStrongholdPieceData::PrisonHall {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::LeftTurn { entry_door } => {
                PersistentStrongholdPieceData::LeftTurn {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::RightTurn { entry_door } => {
                PersistentStrongholdPieceData::RightTurn {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::RoomCrossing {
                entry_door,
                crossing_type,
            } => PersistentStrongholdPieceData::RoomCrossing {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                crossing_type,
            },
            StrongholdPieceData::StraightStairsDown { entry_door } => {
                PersistentStrongholdPieceData::StraightStairsDown {
                    entry_door: Self::stronghold_door_to_persistent(entry_door),
                }
            }
            StrongholdPieceData::StairsDown {
                entry_door,
                is_source,
            } => PersistentStrongholdPieceData::StairsDown {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                is_source,
            },
            StrongholdPieceData::FiveCrossing {
                entry_door,
                left_low,
                left_high,
                right_low,
                right_high,
            } => PersistentStrongholdPieceData::FiveCrossing {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                left_low,
                left_high,
                right_low,
                right_high,
            },
            StrongholdPieceData::ChestCorridor {
                entry_door,
                has_placed_chest,
            } => PersistentStrongholdPieceData::ChestCorridor {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                has_placed_chest,
            },
            StrongholdPieceData::Library {
                entry_door,
                is_tall,
            } => PersistentStrongholdPieceData::Library {
                entry_door: Self::stronghold_door_to_persistent(entry_door),
                is_tall,
            },
            StrongholdPieceData::PortalRoom { has_placed_spawner } => {
                PersistentStrongholdPieceData::PortalRoom { has_placed_spawner }
            }
            StrongholdPieceData::FillerCorridor { steps } => {
                PersistentStrongholdPieceData::FillerCorridor { steps }
            }
        }
    }

    const fn persistent_to_stronghold_piece_data(
        data: &PersistentStrongholdPieceData,
    ) -> StrongholdPieceData {
        match data {
            PersistentStrongholdPieceData::Straight {
                entry_door,
                left_child,
                right_child,
            } => StrongholdPieceData::Straight {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                left_child: *left_child,
                right_child: *right_child,
            },
            PersistentStrongholdPieceData::PrisonHall { entry_door } => {
                StrongholdPieceData::PrisonHall {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::LeftTurn { entry_door } => {
                StrongholdPieceData::LeftTurn {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::RightTurn { entry_door } => {
                StrongholdPieceData::RightTurn {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::RoomCrossing {
                entry_door,
                crossing_type,
            } => StrongholdPieceData::RoomCrossing {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                crossing_type: *crossing_type,
            },
            PersistentStrongholdPieceData::StraightStairsDown { entry_door } => {
                StrongholdPieceData::StraightStairsDown {
                    entry_door: Self::persistent_to_stronghold_door(entry_door),
                }
            }
            PersistentStrongholdPieceData::StairsDown {
                entry_door,
                is_source,
            } => StrongholdPieceData::StairsDown {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                is_source: *is_source,
            },
            PersistentStrongholdPieceData::FiveCrossing {
                entry_door,
                left_low,
                left_high,
                right_low,
                right_high,
            } => StrongholdPieceData::FiveCrossing {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                left_low: *left_low,
                left_high: *left_high,
                right_low: *right_low,
                right_high: *right_high,
            },
            PersistentStrongholdPieceData::ChestCorridor {
                entry_door,
                has_placed_chest,
            } => StrongholdPieceData::ChestCorridor {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                has_placed_chest: *has_placed_chest,
            },
            PersistentStrongholdPieceData::Library {
                entry_door,
                is_tall,
            } => StrongholdPieceData::Library {
                entry_door: Self::persistent_to_stronghold_door(entry_door),
                is_tall: *is_tall,
            },
            PersistentStrongholdPieceData::PortalRoom { has_placed_spawner } => {
                StrongholdPieceData::PortalRoom {
                    has_placed_spawner: *has_placed_spawner,
                }
            }
            PersistentStrongholdPieceData::FillerCorridor { steps } => {
                StrongholdPieceData::FillerCorridor { steps: *steps }
            }
        }
    }

    pub(super) fn mineshaft_kind_to_persistent(
        kind: &MineshaftPieceKind,
    ) -> PersistentMineshaftPieceKind {
        match kind {
            MineshaftPieceKind::Room {
                child_entrance_boxes,
            } => PersistentMineshaftPieceKind::Room {
                child_entrance_boxes: child_entrance_boxes
                    .iter()
                    .map(|&b| PersistentBoundingBox::from_bounding_box(b))
                    .collect(),
            },
            MineshaftPieceKind::Corridor {
                has_rails,
                spider_corridor,
                has_placed_spider,
                num_sections,
            } => PersistentMineshaftPieceKind::Corridor {
                has_rails: *has_rails,
                spider_corridor: *spider_corridor,
                has_placed_spider: *has_placed_spider,
                num_sections: *num_sections,
            },
            MineshaftPieceKind::Crossing {
                direction,
                is_two_floored,
            } => PersistentMineshaftPieceKind::Crossing {
                direction: direction_to_2d(Some(*direction)),
                is_two_floored: *is_two_floored,
            },
            MineshaftPieceKind::Stairs => PersistentMineshaftPieceKind::Stairs,
        }
    }

    pub(super) fn persistent_to_mineshaft_kind(
        kind: &PersistentMineshaftPieceKind,
    ) -> MineshaftPieceKind {
        match kind {
            PersistentMineshaftPieceKind::Room {
                child_entrance_boxes,
            } => MineshaftPieceKind::Room {
                child_entrance_boxes: child_entrance_boxes
                    .iter()
                    .map(|b| b.to_bounding_box())
                    .collect(),
            },
            PersistentMineshaftPieceKind::Corridor {
                has_rails,
                spider_corridor,
                has_placed_spider,
                num_sections,
            } => MineshaftPieceKind::Corridor {
                has_rails: *has_rails,
                spider_corridor: *spider_corridor,
                has_placed_spider: *has_placed_spider,
                num_sections: *num_sections,
            },
            PersistentMineshaftPieceKind::Crossing {
                direction,
                is_two_floored,
            } => MineshaftPieceKind::Crossing {
                direction: required_direction_from_2d(*direction),
                is_two_floored: *is_two_floored,
            },
            PersistentMineshaftPieceKind::Stairs => MineshaftPieceKind::Stairs,
        }
    }

    pub(super) fn structure_piece_payload_to_persistent(
        payload: &StructurePiecePayload,
    ) -> PersistentStructurePiecePayload {
        match payload {
            StructurePiecePayload::Jigsaw(data) => {
                PersistentStructurePiecePayload::Jigsaw(Self::jigsaw_piece_data_to_persistent(data))
            }
            StructurePiecePayload::Template(data) => {
                PersistentStructurePiecePayload::Template(PersistentTemplatePieceData {
                    template_id: data.template_id.clone(),
                    template_position: [
                        data.template_position.x,
                        data.template_position.y,
                        data.template_position.z,
                    ],
                    rotation: rotation_to_persistent(data.rotation),
                    mirror: mirror_to_persistent(data.mirror),
                    rotation_pivot: [
                        data.rotation_pivot.x,
                        data.rotation_pivot.y,
                        data.rotation_pivot.z,
                    ],
                    block_ignore: block_ignore_to_persistent(data.block_ignore),
                    late_block_ignore: block_ignore_to_persistent(data.late_block_ignore),
                    processors: Self::template_processors_to_persistent(&data.processors),
                    liquid_settings: liquid_settings_to_persistent(data.liquid_settings),
                    marker_handling: marker_handling_to_persistent(data.marker_handling),
                    placement_adjustment: placement_adjustment_to_persistent(
                        data.placement_adjustment,
                    ),
                    placement_clip: placement_clip_to_persistent(data.placement_clip),
                    post_process: post_process_to_persistent(data.post_process),
                })
            }
            StructurePiecePayload::Procedural(data) => PersistentStructurePiecePayload::Procedural(
                Self::procedural_piece_data_to_persistent(data),
            ),
        }
    }

    pub(super) fn persistent_to_structure_piece_payload(
        payload: &PersistentStructurePiecePayload,
    ) -> StructurePiecePayload {
        match payload {
            PersistentStructurePiecePayload::Jigsaw(data) => {
                StructurePiecePayload::Jigsaw(Self::persistent_to_jigsaw_piece_data(data))
            }
            PersistentStructurePiecePayload::Template(data) => {
                StructurePiecePayload::Template(TemplatePieceData {
                    template_id: data.template_id.clone(),
                    template_position: IVec3::new(
                        data.template_position[0],
                        data.template_position[1],
                        data.template_position[2],
                    ),
                    rotation: rotation_from_persistent(data.rotation),
                    mirror: mirror_from_persistent(data.mirror),
                    rotation_pivot: IVec3::new(
                        data.rotation_pivot[0],
                        data.rotation_pivot[1],
                        data.rotation_pivot[2],
                    ),
                    block_ignore: block_ignore_from_persistent(data.block_ignore),
                    late_block_ignore: block_ignore_from_persistent(data.late_block_ignore),
                    processors: Self::persistent_to_template_processors(&data.processors),
                    liquid_settings: liquid_settings_from_persistent(data.liquid_settings),
                    marker_handling: marker_handling_from_persistent(data.marker_handling),
                    placement_adjustment: placement_adjustment_from_persistent(
                        &data.placement_adjustment,
                    ),
                    placement_clip: placement_clip_from_persistent(data.placement_clip),
                    post_process: post_process_from_persistent(data.post_process),
                })
            }
            PersistentStructurePiecePayload::Procedural(data) => {
                StructurePiecePayload::Procedural(Self::persistent_to_procedural_piece_data(data))
            }
        }
    }

    pub(super) fn pool_element_to_persistent(element: &PoolElement) -> PersistentPoolElement {
        match element {
            PoolElement::Single {
                location,
                processors,
                projection,
            } => PersistentPoolElement::Single {
                location: location.clone(),
                processors: Self::processors_to_persistent(processors),
                projection: projection_to_persistent(Some(*projection)),
            },
            PoolElement::LegacySingle {
                location,
                processors,
                projection,
            } => PersistentPoolElement::LegacySingle {
                location: location.clone(),
                processors: Self::processors_to_persistent(processors),
                projection: projection_to_persistent(Some(*projection)),
            },
            PoolElement::Empty => PersistentPoolElement::Empty,
            PoolElement::Feature {
                feature,
                projection,
            } => PersistentPoolElement::Feature {
                feature: feature.clone(),
                projection: projection_to_persistent(Some(*projection)),
            },
            PoolElement::List {
                elements,
                projection,
            } => PersistentPoolElement::List {
                elements: elements
                    .iter()
                    .map(Self::pool_element_to_persistent)
                    .collect(),
                projection: projection_to_persistent(Some(*projection)),
            },
        }
    }

    pub(super) fn persistent_to_pool_element(element: &PersistentPoolElement) -> PoolElement {
        match element {
            PersistentPoolElement::Single {
                location,
                processors,
                projection,
            } => PoolElement::Single {
                location: location.clone(),
                processors: Self::persistent_to_processors(processors),
                projection: required_projection_from_persistent(*projection),
            },
            PersistentPoolElement::LegacySingle {
                location,
                processors,
                projection,
            } => PoolElement::LegacySingle {
                location: location.clone(),
                processors: Self::persistent_to_processors(processors),
                projection: required_projection_from_persistent(*projection),
            },
            PersistentPoolElement::Empty => PoolElement::Empty,
            PersistentPoolElement::Feature {
                feature,
                projection,
            } => PoolElement::Feature {
                feature: feature.clone(),
                projection: required_projection_from_persistent(*projection),
            },
            PersistentPoolElement::List {
                elements,
                projection,
            } => PoolElement::List {
                elements: elements
                    .iter()
                    .map(Self::persistent_to_pool_element)
                    .collect(),
                projection: required_projection_from_persistent(*projection),
            },
        }
    }

    pub(super) fn processors_to_persistent(processors: &ProcessorList) -> PersistentProcessorList {
        match processors {
            ProcessorList::Empty => PersistentProcessorList::Empty,
            ProcessorList::Registry(id) => PersistentProcessorList::Registry(id.clone()),
        }
    }

    pub(super) fn persistent_to_processors(processors: &PersistentProcessorList) -> ProcessorList {
        match processors {
            PersistentProcessorList::Empty => ProcessorList::Empty,
            PersistentProcessorList::Registry(id) => ProcessorList::Registry(id.clone()),
        }
    }

    pub(super) fn template_processors_to_persistent(
        processors: &TemplateProcessorList,
    ) -> PersistentTemplateProcessorList {
        match processors {
            TemplateProcessorList::Empty => PersistentTemplateProcessorList::Empty,
            TemplateProcessorList::Registry(id) => {
                PersistentTemplateProcessorList::Registry(id.clone())
            }
            TemplateProcessorList::OceanRuin {
                biome_temp,
                integrity,
            } => PersistentTemplateProcessorList::OceanRuin {
                biome_temp: ocean_ruin_biome_temp_to_persistent(*biome_temp),
                integrity: *integrity,
            },
            TemplateProcessorList::RuinedPortal {
                vertical_placement,
                properties,
            } => PersistentTemplateProcessorList::RuinedPortal {
                vertical_placement: ruined_portal_placement_to_persistent(*vertical_placement),
                cold: properties.cold,
                mossiness: properties.mossiness,
                air_pocket: properties.air_pocket,
                overgrown: properties.overgrown,
                vines: properties.vines,
                replace_with_blackstone: properties.replace_with_blackstone,
            },
        }
    }

    pub(super) fn persistent_to_template_processors(
        processors: &PersistentTemplateProcessorList,
    ) -> TemplateProcessorList {
        match processors {
            PersistentTemplateProcessorList::Empty => TemplateProcessorList::Empty,
            PersistentTemplateProcessorList::Registry(id) => {
                TemplateProcessorList::Registry(id.clone())
            }
            PersistentTemplateProcessorList::OceanRuin {
                biome_temp,
                integrity,
            } => TemplateProcessorList::OceanRuin {
                biome_temp: ocean_ruin_biome_temp_from_persistent(*biome_temp),
                integrity: *integrity,
            },
            PersistentTemplateProcessorList::RuinedPortal {
                vertical_placement,
                cold,
                mossiness,
                air_pocket,
                overgrown,
                vines,
                replace_with_blackstone,
            } => TemplateProcessorList::RuinedPortal {
                vertical_placement: ruined_portal_placement_from_persistent(*vertical_placement),
                properties: RuinedPortalProperties {
                    cold: *cold,
                    mossiness: *mossiness,
                    air_pocket: *air_pocket,
                    overgrown: *overgrown,
                    vines: *vines,
                    replace_with_blackstone: *replace_with_blackstone,
                },
            },
        }
    }

    /// Converts structure starts to persistent format for saving.
    pub(super) fn structure_starts_to_persistent(
        starts: &StructureStartMap,
    ) -> Vec<PersistentStructureStart> {
        let mut persistent: Vec<_> = starts
            .values()
            .filter(|start| !start.pieces.is_empty())
            .map(|start| PersistentStructureStart {
                structure: start.structure.clone(),
                chunk_x: start.chunk_pos.0.x,
                chunk_z: start.chunk_pos.0.y,
                references: start.references,
                pieces: start
                    .pieces
                    .iter()
                    .map(|piece| PersistentStructurePiece {
                        piece_type: piece.piece_type.clone(),
                        bounding_box: PersistentBoundingBox::from_bounding_box(piece.bounding_box),
                        gen_depth: piece.gen_depth,
                        orientation: direction_to_2d(piece.orientation),
                        payload: Self::structure_piece_payload_to_persistent(&piece.payload),
                        ground_level_delta: piece.ground_level_delta,
                        projection: projection_to_persistent(piece.projection),
                        junctions: piece
                            .junctions
                            .iter()
                            .map(|junction| PersistentJigsawJunction {
                                source_x: junction.source_pos.x,
                                source_ground_y: junction.source_pos.y,
                                source_z: junction.source_pos.z,
                                delta_y: junction.delta_y,
                                dest_projection: projection_to_persistent(Some(
                                    junction.dest_projection,
                                )),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();

        persistent.sort_by(|a, b| compare_identifiers(&a.structure, &b.structure));
        persistent
    }

    /// Converts structure references to persistent format for saving.
    pub(super) fn structure_references_to_persistent(
        refs: &StructureReferenceMap,
    ) -> Vec<PersistentStructureReference> {
        let mut persistent: Vec<_> = refs
            .iter()
            .filter(|(_, positions)| !positions.is_empty())
            .map(|(structure, positions)| PersistentStructureReference {
                structure: structure.clone(),
                references: {
                    let packed: Vec<_> = positions
                        .insertion_order_iter()
                        .copied()
                        .map(PackedChunkPos::from)
                        .collect();
                    packed
                },
            })
            .collect();

        persistent.sort_by(|a, b| compare_identifiers(&a.structure, &b.structure));
        persistent
    }

    /// Reconstructs structure starts from persistent data.
    pub(super) fn persistent_to_structure_starts(
        persistent: &[PersistentStructureStart],
    ) -> StructureStartMap {
        persistent
            .iter()
            .map(|ps| {
                let pieces = ps
                    .pieces
                    .iter()
                    .map(|pp| StructurePiece {
                        piece_type: pp.piece_type.clone(),
                        bounding_box: pp.bounding_box.to_bounding_box(),
                        gen_depth: pp.gen_depth,
                        orientation: direction_from_2d(pp.orientation),
                        payload: Self::persistent_to_structure_piece_payload(&pp.payload),
                        ground_level_delta: pp.ground_level_delta,
                        junctions: pp
                            .junctions
                            .iter()
                            .map(|junction| JigsawJunction {
                                source_pos: IVec3::new(
                                    junction.source_x,
                                    junction.source_ground_y,
                                    junction.source_z,
                                ),
                                delta_y: junction.delta_y,
                                dest_projection: required_projection_from_persistent(
                                    junction.dest_projection,
                                ),
                            })
                            .collect(),
                        projection: projection_from_persistent(pp.projection),
                    })
                    .collect();

                let terrain_adjustment = REGISTRY
                    .structures
                    .by_key(&ps.structure)
                    .map_or(TerrainAdjustment::None, |structure| {
                        structure.terrain_adjustment
                    });
                let mut start = StructureStart::new(
                    ps.structure.clone(),
                    ChunkPos::new(ps.chunk_x, ps.chunk_z),
                    pieces,
                    terrain_adjustment,
                );
                start.references = ps.references;
                (ps.structure.clone(), start)
            })
            .collect()
    }

    /// Reconstructs structure references from persistent data.
    pub(super) fn persistent_to_structure_references(
        persistent: &[PersistentStructureReference],
    ) -> StructureReferenceMap {
        persistent
            .iter()
            .map(|pr| {
                let positions = pr
                    .references
                    .iter()
                    .map(|&packed| packed.to_chunk_pos())
                    .collect();
                (pr.structure.clone(), positions)
            })
            .collect()
    }
}
