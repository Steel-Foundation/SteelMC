//! Point of Interest (POI) system.
//!
//! Tracks special blocks (beds, workstations, bells, nether portals, etc.)
//! for efficient spatial queries without scanning every block.

pub mod poi_instance;
pub mod poi_set;
pub mod poi_storage;

pub use poi_instance::PointOfInterest;
pub use poi_set::PointOfInterestSet;
pub use poi_storage::{OccupationStatus, PointOfInterestStorage};
