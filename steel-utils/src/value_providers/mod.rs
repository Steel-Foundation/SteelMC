//! Vanilla-compatible value providers used by world generation and features.

mod float;
mod height;
mod int;
mod vertical_anchor;

pub use float::FloatProvider;
pub use height::HeightProvider;
pub use int::{IntProvider, UniformIntProvider, WeightedIntProvider};
pub use vertical_anchor::VerticalAnchor;

#[cfg(test)]
mod tests;
