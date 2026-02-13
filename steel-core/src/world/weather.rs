#[allow(clippy::struct_field_names)] // thats just how they are called
#[derive(Debug, Default)]
pub struct Weather {
    pub rain_level: f32,
    pub previous_rain_level: f32,
    pub thunder_level: f32,
    pub previous_thunder_level: f32,
}
