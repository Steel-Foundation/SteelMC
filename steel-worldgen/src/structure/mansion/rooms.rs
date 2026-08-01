use super::{Direction, IVec3, LegacyRandom, Rotation};
use super::{
    placement::get_room_name,
    template::{
        MansionTemplatePiece, Mirror, add_piece, compose_rotation, relative, zero_pos_transform,
    },
};

pub(super) fn add_room_1x1(
    pieces: &mut Vec<MansionTemplatePiece>,
    room_pos: IVec3,
    rotation: Rotation,
    door_dir: Option<Direction>,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    let mut piece_rot = Rotation::None;
    let kind;
    match door_dir {
        Some(Direction::East) => kind = "1x1",
        Some(Direction::North) => {
            piece_rot = Rotation::CounterClockwise90;
            kind = "1x1";
        }
        Some(Direction::West) => {
            piece_rot = Rotation::Clockwise180;
            kind = "1x1";
        }
        Some(Direction::South) => {
            piece_rot = Rotation::Clockwise90;
            kind = "1x1";
        }
        _ => kind = "1x1s",
    }
    let name = get_room_name(rng, floor, kind, false);
    let orient = zero_pos_transform(IVec3::new(1, 0, 0), piece_rot, 7, 7);
    piece_rot = compose_rotation(piece_rot, rotation);
    let orient = rotation.transform_pos(orient, IVec3::ZERO);
    let pos = room_pos + IVec3::new(orient.x, 0, orient.z);
    add_piece(pieces, name, pos, piece_rot, Mirror::None);
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla's MansionRoom1x2 constructor surface"
)]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive (door_dir × room_dir) dispatch mirroring vanilla's MansionRoom1x2"
)]
pub(super) fn add_room_1x2(
    pieces: &mut Vec<MansionTemplatePiece>,
    room_pos: IVec3,
    rotation: Rotation,
    room_dir: Direction,
    door_dir: Direction,
    floor: usize,
    is_stairs: bool,
    rng: &mut LegacyRandom,
) {
    let (pos, rot, mirror, kind) = match (door_dir, room_dir) {
        (Direction::East, Direction::South) => (
            relative(room_pos, rotation, Direction::East, 1),
            rotation,
            Mirror::None,
            "1x2side",
        ),
        (Direction::East, Direction::North) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::South,
                6,
            );
            (p, rotation, Mirror::LeftRight, "1x2side")
        }
        (Direction::West, Direction::North) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 7),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::Clockwise180),
                Mirror::None,
                "1x2side",
            )
        }
        (Direction::West, Direction::South) => (
            relative(room_pos, rotation, Direction::East, 7),
            rotation,
            Mirror::FrontBack,
            "1x2side",
        ),
        (Direction::South, Direction::East) => (
            relative(room_pos, rotation, Direction::East, 1),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::LeftRight,
            "1x2side",
        ),
        (Direction::South, Direction::West) => (
            relative(room_pos, rotation, Direction::East, 7),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
            "1x2side",
        ),
        (Direction::North, Direction::West) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 7),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::Clockwise90),
                Mirror::FrontBack,
                "1x2side",
            )
        }
        (Direction::North, Direction::East) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::CounterClockwise90),
                Mirror::None,
                "1x2side",
            )
        }
        (Direction::South, Direction::North) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::North,
                8,
            );
            (p, rotation, Mirror::None, "1x2front")
        }
        (Direction::North, Direction::South) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 7),
                rotation,
                Direction::South,
                14,
            );
            (
                p,
                compose_rotation(rotation, Rotation::Clockwise180),
                Mirror::None,
                "1x2front",
            )
        }
        (Direction::West, Direction::East) => (
            relative(room_pos, rotation, Direction::East, 15),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
            "1x2front",
        ),
        (Direction::East, Direction::West) => {
            let p = relative(
                relative(room_pos, rotation, Direction::West, 7),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::CounterClockwise90),
                Mirror::None,
                "1x2front",
            )
        }
        (Direction::Up, Direction::East) => (
            relative(room_pos, rotation, Direction::East, 15),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
            "1x2secret",
        ),
        (Direction::Up, Direction::South) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::North,
                0,
            );
            (p, rotation, Mirror::None, "1x2secret")
        }
        _ => return,
    };

    let name = get_room_name(rng, floor, kind, is_stairs);
    add_piece(pieces, name, pos, rot, mirror);
}

pub(super) fn add_room_2x2(
    pieces: &mut Vec<MansionTemplatePiece>,
    room_pos: IVec3,
    rotation: Rotation,
    room_dir: Direction,
    door_dir: Direction,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    let (east, south, rot, mirror) = match (door_dir, room_dir) {
        (Direction::East, Direction::South) => (-7, 0, rotation, Mirror::None),
        (Direction::East, Direction::North) => (-7, 6, rotation, Mirror::LeftRight),
        (Direction::North, Direction::East) => (
            1,
            14,
            compose_rotation(rotation, Rotation::CounterClockwise90),
            Mirror::None,
        ),
        (Direction::North, Direction::West) => (
            7,
            14,
            compose_rotation(rotation, Rotation::CounterClockwise90),
            Mirror::LeftRight,
        ),
        (Direction::South, Direction::West) => (
            7,
            -8,
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
        ),
        (Direction::South, Direction::East) => (
            1,
            -8,
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::LeftRight,
        ),
        (Direction::West, Direction::North) => (
            15,
            6,
            compose_rotation(rotation, Rotation::Clockwise180),
            Mirror::None,
        ),
        (Direction::West, Direction::South) => (15, 0, rotation, Mirror::FrontBack),
        _ => return,
    };

    let pos = relative(
        relative(room_pos, rotation, Direction::East, east),
        rotation,
        Direction::South,
        south,
    );
    let name = get_room_name(rng, floor, "2x2", false);
    add_piece(pieces, name, pos, rot, mirror);
}

pub(super) fn add_room_2x2_secret(
    pieces: &mut Vec<MansionTemplatePiece>,
    room_pos: IVec3,
    rotation: Rotation,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    let pos = relative(room_pos, rotation, Direction::East, 1);
    let name = get_room_name(rng, floor, "2x2secret", false);
    add_piece(pieces, name, pos, rotation, Mirror::None);
}
