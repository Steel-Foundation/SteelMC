#[expect(
    clippy::struct_field_names,
    reason = "field names match vanilla weather state naming"
)]
#[derive(Debug, Default)]
pub struct Weather {
    pub rain_level: f32,
    pub previous_rain_level: f32,
    pub thunder_level: f32,
    pub previous_thunder_level: f32,
}
