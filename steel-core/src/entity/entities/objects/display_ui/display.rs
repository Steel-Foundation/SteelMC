//! Vanilla's abstract `Display` implementation.

use simdnbt::owned::NbtCompound;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use steel_registry::entity_data::{Quaternionf, Vector3f};
use steel_registry::vanilla_entity_data::DisplayEntityData;
use steel_utils::BoundingBox;
use steel_utils::locks::SyncMutex;
use crate::entity::Entity;

#[derive(Debug, Clone, Copy)]
struct DisplayState {
    interpolation_duration: i32,
    last_progress: f32,
    culling_bounding_box: BoundingBox,
    update_render_state: bool,
    update_start_tick: bool,
    update_interpolation_duration: bool
}

struct DisplayBase {
    state: SyncMutex<DisplayState>
}

pub enum BillboardConstraints {
    Fixed,
    Vertical,
    Horizontal,
    Center
}

/// A structure describing an affine transformation in 3D space.
///
/// Transformations are applied in the following order:
/// `translation` -> `left_rotation` -> `scale` -> `right_rotation`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transformation {
    /// The translation (displacement) applied by this transformation.
    pub translation: Vector3f,
    /// The left rotation applied by this transformation.
    pub left_rotation: Quaternionf,
    /// The scale applied by this transformation.
    pub scale: Vector3f,
    /// The right rotation applied by this transformation.
    pub right_rotation: Quaternionf
}

impl Transformation {
    /// The identity [`Transformation`].
    pub const IDENTITY: Self = Transformation {
        translation: Vector3f::ZERO,
        left_rotation: Quaternionf::IDENTITY,
        scale: Vector3f::ONE,
        right_rotation: Quaternionf::IDENTITY,
    };
}

/// The abstract display trait, used by block, item and display entities.
///
/// TODO: Write special properties
pub trait Display: Entity {
    fn tick_display(&self) {
        /// TODO: Implement dismounting if the vehicle of the display entity is removed.
    }

    fn display_entity_data(&self) -> &DisplayEntityData;

    fn display_entity_data_mut(&self) -> &mut DisplayEntityData;

    fn set_transformation(&self, transformation: Transformation) {
        let data = self.display_entity_data_mut();

        data.translation.set(transformation.translation);
        data.left_rotation.set(transformation.left_rotation);
        data.scale.set(transformation.scale);
        data.right_rotation.set(transformation.right_rotation);
    }

    fn load_additional_display(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {

    }

    fn save_additional(&self, nbt: &mut NbtCompound) {

    }
}