use super::{Direction, IVec3, Rotation};
use super::{
    grid::{SimpleGrid, is_house},
    template::{MansionTemplatePiece, Mirror, above, add_piece, compose_rotation, relative},
};

#[expect(
    clippy::too_many_lines,
    reason = "mirrors vanilla's MansionPiecePlacer.createRoof inline traversal"
)]
pub(super) fn create_roof(
    pieces: &mut Vec<MansionTemplatePiece>,
    roof_origin: IVec3,
    rotation: Rotation,
    grid: &SimpleGrid,
    above_grid: Option<&SimpleGrid>,
    start_x: i32,
    start_y: i32,
) {
    for y in 0..grid.height {
        for x in 0..grid.width {
            let mut pos = relative(
                roof_origin,
                rotation,
                Direction::South,
                8 + (y - start_y) * 8,
            );
            pos = relative(pos, rotation, Direction::East, (x - start_x) * 8);
            let is_above = above_grid.is_some_and(|g| is_house(g, x, y));

            if is_house(grid, x, y) && !is_above {
                add_piece(pieces, "roof", above(pos, 3), rotation, Mirror::None);

                if !is_house(grid, x + 1, y) {
                    let p = relative(pos, rotation, Direction::East, 6);
                    add_piece(pieces, "roof_front", p, rotation, Mirror::None);
                }
                if !is_house(grid, x - 1, y) {
                    let p = relative(
                        relative(pos, rotation, Direction::East, 0),
                        rotation,
                        Direction::South,
                        7,
                    );
                    add_piece(
                        pieces,
                        "roof_front",
                        p,
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y - 1) {
                    let p = relative(pos, rotation, Direction::West, 1);
                    add_piece(
                        pieces,
                        "roof_front",
                        p,
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y + 1) {
                    let p = relative(
                        relative(pos, rotation, Direction::East, 6),
                        rotation,
                        Direction::South,
                        6,
                    );
                    add_piece(
                        pieces,
                        "roof_front",
                        p,
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    );
                }
            }
        }
    }

    if let Some(above_g) = above_grid {
        for y in 0..grid.height {
            for x in 0..grid.width {
                let mut pos = relative(
                    roof_origin,
                    rotation,
                    Direction::South,
                    8 + (y - start_y) * 8,
                );
                pos = relative(pos, rotation, Direction::East, (x - start_x) * 8);
                let is_above = is_house(above_g, x, y);
                if !is_house(grid, x, y) || !is_above {
                    continue;
                }

                if !is_house(grid, x + 1, y) {
                    let p = relative(pos, rotation, Direction::East, 7);
                    add_piece(pieces, "small_wall", p, rotation, Mirror::None);
                }
                if !is_house(grid, x - 1, y) {
                    let p = relative(
                        relative(pos, rotation, Direction::West, 1),
                        rotation,
                        Direction::South,
                        6,
                    );
                    add_piece(
                        pieces,
                        "small_wall",
                        p,
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y - 1) {
                    let p = relative(
                        relative(pos, rotation, Direction::West, 0),
                        rotation,
                        Direction::North,
                        1,
                    );
                    add_piece(
                        pieces,
                        "small_wall",
                        p,
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y + 1) {
                    let p = relative(
                        relative(pos, rotation, Direction::East, 6),
                        rotation,
                        Direction::South,
                        7,
                    );
                    add_piece(
                        pieces,
                        "small_wall",
                        p,
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    );
                }

                if !is_house(grid, x + 1, y) {
                    if !is_house(grid, x, y - 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::East, 7),
                            rotation,
                            Direction::North,
                            2,
                        );
                        add_piece(pieces, "small_wall_corner", p, rotation, Mirror::None);
                    }
                    if !is_house(grid, x, y + 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::East, 8),
                            rotation,
                            Direction::South,
                            7,
                        );
                        add_piece(
                            pieces,
                            "small_wall_corner",
                            p,
                            compose_rotation(rotation, Rotation::Clockwise90),
                            Mirror::None,
                        );
                    }
                }
                if !is_house(grid, x - 1, y) {
                    if !is_house(grid, x, y - 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::West, 2),
                            rotation,
                            Direction::North,
                            1,
                        );
                        add_piece(
                            pieces,
                            "small_wall_corner",
                            p,
                            compose_rotation(rotation, Rotation::CounterClockwise90),
                            Mirror::None,
                        );
                    }
                    if !is_house(grid, x, y + 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::West, 1),
                            rotation,
                            Direction::South,
                            8,
                        );
                        add_piece(
                            pieces,
                            "small_wall_corner",
                            p,
                            compose_rotation(rotation, Rotation::Clockwise180),
                            Mirror::None,
                        );
                    }
                }
            }
        }
    }

    for y in 0..grid.height {
        for x in 0..grid.width {
            let mut pos = relative(
                roof_origin,
                rotation,
                Direction::South,
                8 + (y - start_y) * 8,
            );
            pos = relative(pos, rotation, Direction::East, (x - start_x) * 8);
            let is_above = above_grid.is_some_and(|g| is_house(g, x, y));
            if !is_house(grid, x, y) || is_above {
                continue;
            }

            if !is_house(grid, x + 1, y) {
                let p = relative(pos, rotation, Direction::East, 6);
                if !is_house(grid, x, y + 1) {
                    let p2 = relative(p, rotation, Direction::South, 6);
                    add_piece(pieces, "roof_corner", p2, rotation, Mirror::None);
                } else if is_house(grid, x + 1, y + 1) {
                    let p2 = relative(p, rotation, Direction::South, 5);
                    add_piece(pieces, "roof_inner_corner", p2, rotation, Mirror::None);
                }
                if !is_house(grid, x, y - 1) {
                    add_piece(
                        pieces,
                        "roof_corner",
                        p,
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    );
                } else if is_house(grid, x + 1, y - 1) {
                    let p2 = relative(
                        relative(pos, rotation, Direction::East, 9),
                        rotation,
                        Direction::North,
                        2,
                    );
                    add_piece(
                        pieces,
                        "roof_inner_corner",
                        p2,
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    );
                }
            }

            if !is_house(grid, x - 1, y) {
                let p = relative(pos, rotation, Direction::East, 0);
                let p = relative(p, rotation, Direction::South, 0);
                if !is_house(grid, x, y + 1) {
                    let p2 = relative(p, rotation, Direction::South, 6);
                    add_piece(
                        pieces,
                        "roof_corner",
                        p2,
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    );
                } else if is_house(grid, x - 1, y + 1) {
                    let p2 = relative(
                        relative(p, rotation, Direction::South, 8),
                        rotation,
                        Direction::West,
                        3,
                    );
                    add_piece(
                        pieces,
                        "roof_inner_corner",
                        p2,
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    );
                }
                if !is_house(grid, x, y - 1) {
                    add_piece(
                        pieces,
                        "roof_corner",
                        p,
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    );
                } else if is_house(grid, x - 1, y - 1) {
                    let p2 = relative(p, rotation, Direction::South, 1);
                    add_piece(
                        pieces,
                        "roof_inner_corner",
                        p2,
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    );
                }
            }
        }
    }
}
