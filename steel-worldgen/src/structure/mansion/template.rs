use super::{
    BoundingBox, Direction, IVec3, Identifier, LiquidSettingsData, Rotation, StructureBlockIgnore,
    StructureMirror, StructurePiece, StructurePiecePayload, TemplateMarkerHandling,
    TemplatePieceData, TemplatePlacementAdjustment, TemplatePlacementClip, TemplatePostProcess,
    TemplateProcessorList,
};

pub(super) fn template_size(name: &str) -> [i32; 3] {
    match name {
        "entrance" => [21, 19, 16],
        "wall_flat" | "wall_window" => [2, 8, 8],
        "wall_corner" => [9, 8, 2],
        "corridor_floor" => [7, 8, 7],
        "carpet_north" => [5, 1, 2],
        "carpet_east" => [2, 1, 5],
        "carpet_south_1" => [8, 8, 3],
        "carpet_south_2" => [8, 11, 3],
        "carpet_west_1" => [3, 8, 8],
        "carpet_west_2" => [3, 11, 8],
        "indoors_wall_1" | "indoors_door_1" => [1, 8, 8],
        "indoors_wall_2" | "indoors_door_2" => [1, 11, 8],
        "roof" => [8, 1, 8],
        "roof_corner" | "roof_inner_corner" => [4, 4, 4],
        "roof_front" => [4, 4, 8],
        "small_wall" => [2, 4, 8],
        "small_wall_corner" => [2, 4, 2],
        s if s.starts_with("1x1_a") => [7, 8, 7],
        s if s.starts_with("1x1_b") => [7, 11, 7],
        "1x2_c_stairs" | "1x2_d_stairs" => [7, 22, 15],
        s if s.starts_with("1x2_c") || s.starts_with("1x2_d") || s.starts_with("1x2_se") => {
            [7, 11, 15]
        }
        s if s.starts_with("1x2_a") || s.starts_with("1x2_b") || s.starts_with("1x2_s") => {
            [7, 8, 15]
        }
        s if s.starts_with("2x2_a") => [15, 8, 15],
        s if s.starts_with("2x2_b") || s.starts_with("2x2_s") => [15, 11, 15],
        _ => {
            tracing::warn!("Unknown mansion template: {name}, using 1x1x1");
            [1, 1, 1]
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mirror {
    None,
    LeftRight,
    FrontBack,
}

pub(super) fn piece_bb(pos: IVec3, size: IVec3, rotation: Rotation, mirror: Mirror) -> BoundingBox {
    let dx = size.x - 1;
    let dy = size.y - 1;
    let dz = size.z - 1;
    let (x1, z1) = apply_mirror(0, 0, mirror);
    let (x2, z2) = apply_mirror(dx, dz, mirror);
    let c1 = rotation.transform_pos(IVec3::new(x1, 0, z1), IVec3::ZERO);
    let c2 = rotation.transform_pos(IVec3::new(x2, dy, z2), IVec3::ZERO);
    BoundingBox::new(
        IVec3::new(c1.x.min(c2.x), c1.y.min(c2.y), c1.z.min(c2.z)) + pos,
        IVec3::new(c1.x.max(c2.x), c1.y.max(c2.y), c1.z.max(c2.z)) + pos,
    )
}

pub(super) const fn apply_mirror(x: i32, z: i32, mirror: Mirror) -> (i32, i32) {
    match mirror {
        Mirror::None => (x, z),
        Mirror::FrontBack => (-x, z),
        Mirror::LeftRight => (x, -z),
    }
}

pub(super) const fn structure_mirror(mirror: Mirror) -> StructureMirror {
    match mirror {
        Mirror::None => StructureMirror::None,
        Mirror::FrontBack => StructureMirror::FrontBack,
        Mirror::LeftRight => StructureMirror::LeftRight,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct MansionTemplatePiece {
    template_name: String,
    position: IVec3,
    rotation: Rotation,
    mirror: Mirror,
}

impl MansionTemplatePiece {
    pub(super) fn new(
        template_name: impl Into<String>,
        position: IVec3,
        rotation: Rotation,
        mirror: Mirror,
    ) -> Self {
        Self {
            template_name: template_name.into(),
            position,
            rotation,
            mirror,
        }
    }

    pub(super) fn bounding_box(&self) -> BoundingBox {
        piece_bb(
            self.position,
            IVec3::from(template_size(&self.template_name)),
            self.rotation,
            self.mirror,
        )
    }

    pub(super) fn template_id(&self) -> Identifier {
        Identifier::vanilla(format!("woodland_mansion/{}", self.template_name))
    }

    pub(super) fn into_structure_piece(self) -> StructurePiece {
        let bounding_box = self.bounding_box();
        let mirror = structure_mirror(self.mirror);
        let template_id = self.template_id();
        StructurePiece {
            piece_type: Identifier::new_static("minecraft", "wmp"),
            bounding_box,
            gen_depth: 0,
            orientation: Some(Direction::North),
            payload: StructurePiecePayload::Template(TemplatePieceData {
                template_id,
                template_position: self.position,
                rotation: self.rotation,
                mirror,
                rotation_pivot: IVec3::ZERO,
                block_ignore: StructureBlockIgnore::StructureBlock,
                late_block_ignore: StructureBlockIgnore::None,
                processors: TemplateProcessorList::Empty,
                liquid_settings: LiquidSettingsData::ApplyWaterlogging,
                marker_handling: TemplateMarkerHandling::WoodlandMansion,
                placement_adjustment: TemplatePlacementAdjustment::None,
                placement_clip: TemplatePlacementClip::CenterChunk,
                post_process: TemplatePostProcess::None,
            }),
            ground_level_delta: 0,
            junctions: Vec::new(),
            projection: None,
        }
    }
}

pub(super) fn add_piece(
    pieces: &mut Vec<MansionTemplatePiece>,
    template_name: impl Into<String>,
    position: IVec3,
    rotation: Rotation,
    mirror: Mirror,
) {
    pieces.push(MansionTemplatePiece::new(
        template_name,
        position,
        rotation,
        mirror,
    ));
}

pub(super) fn relative(pos: IVec3, rotation: Rotation, dir: Direction, amount: i32) -> IVec3 {
    let rotated = rotation.rotate(dir);
    let offset = rotated.offset_vec();
    pos + offset * amount
}

pub(super) const fn above(pos: IVec3, amount: i32) -> IVec3 {
    IVec3::new(pos.x, pos.y + amount, pos.z)
}

pub(super) const fn zero_pos_transform(
    zero: IVec3,
    rotation: Rotation,
    size_x: i32,
    size_z: i32,
) -> IVec3 {
    let sx = size_x - 1;
    let sz = size_z - 1;
    let (dx, dz) = match rotation {
        Rotation::None => (0, 0),
        Rotation::Clockwise90 => (sz, 0),
        Rotation::Clockwise180 => (sx, sz),
        Rotation::CounterClockwise90 => (0, sx),
    };
    IVec3::new(zero.x + dx, zero.y, zero.z + dz)
}

pub(super) const fn compose_rotation(base: Rotation, add: Rotation) -> Rotation {
    base.then(add)
}

pub(super) const fn dir_from_2d(value: i32) -> Direction {
    match value & 3 {
        0 => Direction::South,
        1 => Direction::West,
        2 => Direction::North,
        _ => Direction::East,
    }
}
