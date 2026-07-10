//! Vanilla command coordinate expressions.

use std::f32::consts::PI;

use glam::DVec3;
use steel_math::trig;
use steel_utils::{BlockPos, translations};
use text_components::{TextComponent, translation::Translation};

use super::CommandSource;
use crate::command::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, ReaderCursor, StringReader, SuggestionsBuilder,
};

/// A position or rotation expressed in world-relative or source-local coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Coordinates {
    World(WorldCoordinates),
    Local(LocalCoordinates),
}

impl Coordinates {
    /// Resolves this expression against the current command source.
    pub(crate) fn position(self, source: &CommandSource) -> DVec3 {
        match self {
            Self::World(coordinates) => coordinates.position(source.position()),
            Self::Local(coordinates) => {
                coordinates.position(source.anchor_position(), source.rotation())
            }
        }
    }

    /// Resolves this expression as a `(yaw, pitch)` rotation.
    pub(crate) fn rotation(self, source: &CommandSource) -> (f32, f32) {
        match self {
            Self::World(coordinates) => coordinates.rotation(source.rotation()),
            Self::Local(_) => (0.0, 0.0),
        }
    }

    /// Resolves this expression to the containing block position.
    pub(crate) fn block_pos(self, source: &CommandSource) -> BlockPos {
        BlockPos::from(self.position(source))
    }

    /// Returns whether the X component used relative or local syntax.
    pub(crate) const fn is_x_relative(self) -> bool {
        match self {
            Self::World(coordinates) => coordinates.x.is_relative(),
            Self::Local(_) => true,
        }
    }

    /// Returns whether the Y component used relative or local syntax.
    pub(crate) const fn is_y_relative(self) -> bool {
        match self {
            Self::World(coordinates) => coordinates.y.is_relative(),
            Self::Local(_) => true,
        }
    }

    /// Returns whether the Z component used relative or local syntax.
    pub(crate) const fn is_z_relative(self) -> bool {
        match self {
            Self::World(coordinates) => coordinates.z.is_relative(),
            Self::Local(_) => true,
        }
    }
}

/// One absolute or source-relative world coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WorldCoordinate {
    relative: bool,
    value: f64,
}

impl WorldCoordinate {
    pub(super) const fn new(relative: bool, value: f64) -> Self {
        Self { relative, value }
    }

    fn resolve(self, origin: f64) -> f64 {
        if self.relative {
            self.value + origin
        } else {
            self.value
        }
    }

    const fn is_relative(self) -> bool {
        self.relative
    }
}

/// Three world-coordinate components retained until command execution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WorldCoordinates {
    x: WorldCoordinate,
    y: WorldCoordinate,
    z: WorldCoordinate,
}

impl WorldCoordinates {
    pub(super) const fn new(x: WorldCoordinate, y: WorldCoordinate, z: WorldCoordinate) -> Self {
        Self { x, y, z }
    }

    fn position(self, origin: DVec3) -> DVec3 {
        DVec3::new(
            self.x.resolve(origin.x),
            self.y.resolve(origin.y),
            self.z.resolve(origin.z),
        )
    }

    fn rotation(self, (origin_yaw, origin_pitch): (f32, f32)) -> (f32, f32) {
        (
            self.y.resolve(f64::from(origin_yaw)) as f32,
            self.x.resolve(f64::from(origin_pitch)) as f32,
        )
    }
}

/// Three source-local components retained until command execution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LocalCoordinates {
    left: f64,
    up: f64,
    forwards: f64,
}

impl LocalCoordinates {
    pub(super) const fn new(left: f64, up: f64, forwards: f64) -> Self {
        Self { left, up, forwards }
    }

    fn position(self, anchor: DVec3, (yaw, pitch): (f32, f32)) -> DVec3 {
        let radians_per_degree = PI / 180.0;
        let y_rotation = (yaw + 90.0) * radians_per_degree;
        let x_rotation = -pitch * radians_per_degree;
        let x_up_rotation = (-pitch + 90.0) * radians_per_degree;
        let y_cos = f64::from(trig::cos(f64::from(y_rotation)));
        let y_sin = f64::from(trig::sin(f64::from(y_rotation)));
        let x_cos = f64::from(trig::cos(f64::from(x_rotation)));
        let x_sin = f64::from(trig::sin(f64::from(x_rotation)));
        let x_cos_up = f64::from(trig::cos(f64::from(x_up_rotation)));
        let x_sin_up = f64::from(trig::sin(f64::from(x_up_rotation)));
        let forwards_axis = DVec3::new(y_cos * x_cos, x_sin, y_sin * x_cos);
        let up_axis = DVec3::new(y_cos * x_cos_up, x_sin_up, y_sin * x_cos_up);
        let left_axis = -forwards_axis.cross(up_axis);

        let offset = forwards_axis * self.forwards + up_axis * self.up + left_axis * self.left;
        offset + anchor
    }
}

pub(super) fn parse_block_pos(
    reader: &mut StringReader<'_>,
) -> Result<Coordinates, CommandSyntaxError> {
    if reader.peek() == Some('^') {
        parse_local_coordinates(reader)
    } else {
        parse_world_coordinates_int(reader)
    }
}

pub(super) fn parse_vec3(
    reader: &mut StringReader<'_>,
    center_integers: bool,
) -> Result<Coordinates, CommandSyntaxError> {
    if reader.peek() == Some('^') {
        parse_local_coordinates(reader)
    } else {
        parse_world_coordinates_double(reader, center_integers)
    }
}

fn parse_world_coordinates_int(
    reader: &mut StringReader<'_>,
) -> Result<Coordinates, CommandSyntaxError> {
    let start = reader.checkpoint();
    let x = parse_world_coordinate_int(reader)?;
    if reader.peek() != Some(' ') {
        reader.restore(start);
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS3D_INCOMPLETE,
        ));
    }
    reader.skip();
    let y = parse_world_coordinate_int(reader)?;
    if reader.peek() != Some(' ') {
        reader.restore(start);
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS3D_INCOMPLETE,
        ));
    }
    reader.skip();
    let z = parse_world_coordinate_int(reader)?;
    Ok(Coordinates::World(WorldCoordinates::new(x, y, z)))
}

fn parse_world_coordinates_double(
    reader: &mut StringReader<'_>,
    center_integers: bool,
) -> Result<Coordinates, CommandSyntaxError> {
    let start = reader.checkpoint();
    let x = parse_world_coordinate_double(reader, center_integers)?;
    if reader.peek() != Some(' ') {
        reader.restore(start);
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS3D_INCOMPLETE,
        ));
    }
    reader.skip();
    let y = parse_world_coordinate_double(reader, false)?;
    if reader.peek() != Some(' ') {
        reader.restore(start);
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS3D_INCOMPLETE,
        ));
    }
    reader.skip();
    let z = parse_world_coordinate_double(reader, center_integers)?;
    Ok(Coordinates::World(WorldCoordinates::new(x, y, z)))
}

fn parse_local_coordinates(
    reader: &mut StringReader<'_>,
) -> Result<Coordinates, CommandSyntaxError> {
    let start = reader.checkpoint();
    let left = parse_local_coordinate_or_restore(reader, start)?;
    if reader.peek() != Some(' ') {
        reader.restore(start);
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS3D_INCOMPLETE,
        ));
    }
    reader.skip();
    let up = parse_local_coordinate_or_restore(reader, start)?;
    if reader.peek() != Some(' ') {
        reader.restore(start);
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS3D_INCOMPLETE,
        ));
    }
    reader.skip();
    let forwards = parse_local_coordinate_or_restore(reader, start)?;
    Ok(Coordinates::Local(LocalCoordinates::new(
        left, up, forwards,
    )))
}

fn parse_local_coordinate_or_restore(
    reader: &mut StringReader<'_>,
    argument_start: ReaderCursor,
) -> Result<f64, CommandSyntaxError> {
    match parse_local_coordinate(reader) {
        Ok(value) => Ok(value),
        Err(LocalCoordinateError::MissingDouble) => Err(translated_error(
            reader,
            &translations::ARGUMENT_POS_MISSING_DOUBLE,
        )),
        Err(LocalCoordinateError::Mixed) => {
            reader.restore(argument_start);
            Err(translated_error(reader, &translations::ARGUMENT_POS_MIXED))
        }
        Err(LocalCoordinateError::Syntax(error)) => Err(error),
    }
}

enum LocalCoordinateError {
    MissingDouble,
    Mixed,
    Syntax(CommandSyntaxError),
}

fn parse_local_coordinate(reader: &mut StringReader<'_>) -> Result<f64, LocalCoordinateError> {
    if !reader.can_read() {
        return Err(LocalCoordinateError::MissingDouble);
    }
    if reader.peek() != Some('^') {
        return Err(LocalCoordinateError::Mixed);
    }
    reader.skip();
    if !reader.can_read() || reader.peek() == Some(' ') {
        return Ok(0.0);
    }
    reader.read_double().map_err(LocalCoordinateError::Syntax)
}

fn parse_world_coordinate_int(
    reader: &mut StringReader<'_>,
) -> Result<WorldCoordinate, CommandSyntaxError> {
    if reader.peek() == Some('^') {
        return Err(translated_error(reader, &translations::ARGUMENT_POS_MIXED));
    }
    if !reader.can_read() {
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS_MISSING_INT,
        ));
    }
    let relative = read_relative_prefix(reader);
    let value = if reader.can_read() && reader.peek() != Some(' ') {
        if relative {
            reader.read_double()?
        } else {
            f64::from(reader.read_int()?)
        }
    } else {
        0.0
    };
    Ok(WorldCoordinate::new(relative, value))
}

fn parse_world_coordinate_double(
    reader: &mut StringReader<'_>,
    center_integer: bool,
) -> Result<WorldCoordinate, CommandSyntaxError> {
    if reader.peek() == Some('^') {
        return Err(translated_error(reader, &translations::ARGUMENT_POS_MIXED));
    }
    if !reader.can_read() {
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_POS_MISSING_DOUBLE,
        ));
    }
    let relative = read_relative_prefix(reader);
    let number_start = reader.read_so_far().len();
    let mut value = if reader.can_read() && reader.peek() != Some(' ') {
        reader.read_double()?
    } else {
        0.0
    };
    let number = &reader.read_so_far()[number_start..];
    if !relative && center_integer && !number.contains('.') {
        value += 0.5;
    }
    Ok(WorldCoordinate::new(relative, value))
}

pub(super) fn parse_rotation(
    reader: &mut StringReader<'_>,
) -> Result<Coordinates, CommandSyntaxError> {
    let start = reader.checkpoint();
    if !reader.can_read() {
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_ROTATION_INCOMPLETE,
        ));
    }
    let yaw = parse_world_coordinate_double(reader, false)?;
    if reader.peek() != Some(' ') {
        reader.restore(start);
        return Err(translated_error(
            reader,
            &translations::ARGUMENT_ROTATION_INCOMPLETE,
        ));
    }
    reader.skip();
    let pitch = parse_world_coordinate_double(reader, false)?;
    Ok(Coordinates::World(WorldCoordinates::new(
        pitch,
        yaw,
        WorldCoordinate::new(true, 0.0),
    )))
}

fn read_relative_prefix(reader: &mut StringReader<'_>) -> bool {
    if reader.peek() != Some('~') {
        return false;
    }
    reader.skip();
    true
}

fn translated_error(reader: &StringReader<'_>, translation: &Translation<0>) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(translation),
    )))
}

pub(super) fn suggest_coordinates(
    builder: &mut SuggestionsBuilder<'_>,
    parser: impl Fn(&mut StringReader<'_>) -> Result<Coordinates, CommandSyntaxError>,
) {
    let input = builder.remaining();
    let coordinate = if input.starts_with('^') { "^" } else { "~" };
    if input.is_empty() {
        let two_coordinates = format!("{coordinate} {coordinate}");
        let full = format!("{two_coordinates} {coordinate}");
        if valid_coordinates(&full, &parser) {
            builder.suggest(coordinate);
            builder.suggest(two_coordinates);
            builder.suggest(full);
        }
        return;
    }

    let mut fields = input.split(' ').collect::<Vec<_>>();
    while fields.last() == Some(&"") {
        fields.pop();
    }
    match fields.as_slice() {
        [x] => {
            let two_coordinates = format!("{x} {coordinate}");
            let full = format!("{two_coordinates} {coordinate}");
            if valid_coordinates(&full, &parser) {
                builder.suggest(two_coordinates);
                builder.suggest(full);
            }
        }
        [x, y] => {
            let full = format!("{x} {y} {coordinate}");
            if valid_coordinates(&full, &parser) {
                builder.suggest(full);
            }
        }
        _ => {}
    }
}

fn valid_coordinates(
    input: &str,
    parser: &impl Fn(&mut StringReader<'_>) -> Result<Coordinates, CommandSyntaxError>,
) -> bool {
    parser(&mut StringReader::new(input)).is_ok()
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::{LocalCoordinates, WorldCoordinate, WorldCoordinates};

    #[test]
    fn world_coordinates_resolve_relative_components_at_execution() {
        let coordinates = WorldCoordinates::new(
            WorldCoordinate::new(true, 2.5),
            WorldCoordinate::new(false, 64.0),
            WorldCoordinate::new(true, -3.0),
        );

        assert_eq!(
            coordinates.position(DVec3::new(10.0, 20.0, 30.0)),
            DVec3::new(12.5, 64.0, 27.0)
        );
    }

    #[test]
    fn rotation_coordinates_keep_vanilla_pitch_yaw_component_order() {
        let coordinates = WorldCoordinates::new(
            WorldCoordinate::new(true, 5.0),
            WorldCoordinate::new(false, 90.0),
            WorldCoordinate::new(true, 0.0),
        );

        assert_eq!(coordinates.rotation((45.0, 10.0)), (90.0, 15.0));
    }

    #[test]
    fn local_coordinates_follow_source_rotation() {
        let coordinates = LocalCoordinates::new(0.0, 0.0, 1.0);
        let position = coordinates.position(DVec3::ZERO, (0.0, 0.0));

        assert!(position.x.abs() < f64::from(f32::EPSILON));
        assert!(position.y.abs() < f64::from(f32::EPSILON));
        assert!((position.z - 1.0).abs() < f64::from(f32::EPSILON));
    }

    #[test]
    fn coordinates_retain_axis_relative_metadata() {
        let coordinates = super::Coordinates::World(WorldCoordinates::new(
            WorldCoordinate::new(true, 0.0),
            WorldCoordinate::new(false, 64.0),
            WorldCoordinate::new(true, 2.0),
        ));
        assert!(coordinates.is_x_relative());
        assert!(!coordinates.is_y_relative());
        assert!(coordinates.is_z_relative());

        let local = super::Coordinates::Local(LocalCoordinates::new(0.0, 0.0, 0.0));
        assert!(local.is_x_relative());
        assert!(local.is_y_relative());
        assert!(local.is_z_relative());
    }
}
