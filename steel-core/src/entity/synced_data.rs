use steel_registry::{entity_data::DataValue, vanilla_entity_data::VanillaEntityData};
use steel_utils::locks::SyncMutex;

use crate::entity::EntitySharedFlags;

/// Thread-safe access to an entity's vanilla synchronized data.
pub trait EntitySyncedData: Send + Sync {
    /// Packs dirty values for network sync, clearing dirty flags.
    fn pack_dirty(&self) -> Option<Vec<DataValue>>;

    /// Packs all non-default values for initial entity spawn.
    fn pack_all(&self) -> Vec<DataValue>;

    /// Returns the shared vanilla `NoGravity` flag.
    fn is_no_gravity(&self) -> bool;

    /// Returns the shared vanilla shift-key-down flag.
    fn is_shift_key_down(&self) -> bool;

    /// Returns the shared vanilla swimming flag.
    fn is_swimming(&self) -> bool;
}

impl<T> EntitySyncedData for SyncMutex<T>
where
    T: VanillaEntityData + Send + Sync,
{
    fn pack_dirty(&self) -> Option<Vec<DataValue>> {
        VanillaEntityData::pack_dirty(&mut *self.lock())
    }

    fn pack_all(&self) -> Vec<DataValue> {
        VanillaEntityData::pack_all(&*self.lock())
    }

    fn is_no_gravity(&self) -> bool {
        *VanillaEntityData::base(&*self.lock()).no_gravity.get()
    }

    fn is_shift_key_down(&self) -> bool {
        EntitySharedFlags::from_metadata_byte(
            *VanillaEntityData::base(&*self.lock()).shared_flags.get(),
        )
        .contains(EntitySharedFlags::SHIFT_KEY_DOWN)
    }

    fn is_swimming(&self) -> bool {
        EntitySharedFlags::from_metadata_byte(
            *VanillaEntityData::base(&*self.lock()).shared_flags.get(),
        )
        .contains(EntitySharedFlags::SWIMMING)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{entity_data::EntityData, vanilla_entity_data::ItemEntityData};

    use super::*;

    #[test]
    fn synced_data_reads_no_gravity_from_generated_base_layer() {
        let data = SyncMutex::new(ItemEntityData::new());
        assert!(!EntitySyncedData::is_no_gravity(&data));

        data.lock().base_mut().no_gravity.set(true);

        assert!(EntitySyncedData::is_no_gravity(&data));
        let Some(values) = EntitySyncedData::pack_dirty(&data) else {
            panic!("expected dirty no-gravity metadata");
        };
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].index, 5);
        assert_eq!(values[0].serializer_id, 8);
        assert!(matches!(values[0].value, EntityData::Boolean(true)));
        assert!(EntitySyncedData::pack_dirty(&data).is_none());
    }

    #[test]
    fn synced_data_reads_shift_key_down_from_generated_base_layer() {
        let data = SyncMutex::new(ItemEntityData::new());
        assert!(!EntitySyncedData::is_shift_key_down(&data));

        data.lock()
            .base_mut()
            .shared_flags
            .set(EntitySharedFlags::SHIFT_KEY_DOWN.metadata_byte());

        assert!(EntitySyncedData::is_shift_key_down(&data));
    }

    #[test]
    fn synced_data_reads_swimming_from_generated_base_layer() {
        let data = SyncMutex::new(ItemEntityData::new());
        assert!(!EntitySyncedData::is_swimming(&data));

        data.lock()
            .base_mut()
            .shared_flags
            .set(EntitySharedFlags::SWIMMING.metadata_byte());

        assert!(EntitySyncedData::is_swimming(&data));
    }
}
