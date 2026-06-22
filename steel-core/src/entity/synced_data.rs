use steel_registry::{
    entity_data::{DataValue, EntityPose},
    vanilla_entity_data::VanillaEntityData,
};
use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use crate::entity::EntitySharedFlags;

/// Thread-safe access to an entity's vanilla synchronized data.
pub trait EntitySyncedData: Send + Sync {
    /// Packs dirty values for network sync, clearing dirty flags.
    fn pack_dirty(&self) -> Option<Vec<DataValue>>;

    /// Packs all non-default values for initial entity spawn.
    fn pack_all(&self) -> Vec<DataValue>;

    /// Returns the shared vanilla `NoGravity` flag.
    fn is_no_gravity(&self) -> bool;

    /// Sets synchronized vanilla air supply.
    fn set_air_supply(&self, air_supply: i32);

    /// Sets synchronized vanilla custom name.
    fn set_custom_name(&self, custom_name: Option<TextComponent>);

    /// Sets synchronized vanilla custom-name visibility.
    fn set_custom_name_visible(&self, visible: bool);

    /// Sets synchronized vanilla silent flag.
    fn set_silent(&self, silent: bool);

    /// Sets the shared vanilla `NoGravity` flag.
    fn set_no_gravity(&self, no_gravity: bool);

    /// Sets synchronized vanilla pose.
    fn set_pose(&self, pose: EntityPose);

    /// Returns the shared vanilla shift-key-down flag.
    fn is_shift_key_down(&self) -> bool;

    /// Returns the shared vanilla swimming flag.
    fn is_swimming(&self) -> bool;

    /// Returns the shared vanilla invisible flag.
    fn is_base_invisible_flag(&self) -> bool;

    /// Sets the shared vanilla shift-key-down flag.
    fn set_shift_key_down(&self, shift_key_down: bool);

    /// Sets the shared vanilla swimming flag.
    fn set_swimming(&self, swimming: bool);

    /// Sets the shared vanilla sprinting flag.
    fn set_sprinting(&self, sprinting: bool);

    /// Sets the shared vanilla fall-flying flag.
    fn set_fall_flying(&self, fall_flying: bool);

    /// Sets the shared vanilla on-fire flag.
    fn set_base_on_fire_flag(&self, on_fire: bool);

    /// Sets the shared vanilla invisible flag.
    fn set_base_invisible_flag(&self, invisible: bool);

    /// Sets the shared vanilla glowing flag.
    fn set_base_glowing_flag(&self, glowing: bool);

    /// Sets synchronized vanilla frozen ticks.
    fn set_base_ticks_frozen(&self, ticks_frozen: i32);
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

    fn set_air_supply(&self, air_supply: i32) {
        VanillaEntityData::base_mut(&mut *self.lock())
            .air_supply
            .set(air_supply);
    }

    fn set_custom_name(&self, custom_name: Option<TextComponent>) {
        VanillaEntityData::base_mut(&mut *self.lock())
            .custom_name
            .set(custom_name.map(Box::new));
    }

    fn set_custom_name_visible(&self, visible: bool) {
        VanillaEntityData::base_mut(&mut *self.lock())
            .custom_name_visible
            .set(visible);
    }

    fn set_silent(&self, silent: bool) {
        VanillaEntityData::base_mut(&mut *self.lock())
            .silent
            .set(silent);
    }

    fn set_no_gravity(&self, no_gravity: bool) {
        VanillaEntityData::base_mut(&mut *self.lock())
            .no_gravity
            .set(no_gravity);
    }

    fn set_pose(&self, pose: EntityPose) {
        VanillaEntityData::base_mut(&mut *self.lock())
            .pose
            .set(pose);
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

    fn is_base_invisible_flag(&self) -> bool {
        EntitySharedFlags::from_metadata_byte(
            *VanillaEntityData::base(&*self.lock()).shared_flags.get(),
        )
        .contains(EntitySharedFlags::INVISIBLE)
    }

    fn set_shift_key_down(&self, shift_key_down: bool) {
        self.set_shared_flag(EntitySharedFlags::SHIFT_KEY_DOWN, shift_key_down);
    }

    fn set_swimming(&self, swimming: bool) {
        self.set_shared_flag(EntitySharedFlags::SWIMMING, swimming);
    }

    fn set_sprinting(&self, sprinting: bool) {
        self.set_shared_flag(EntitySharedFlags::SPRINTING, sprinting);
    }

    fn set_fall_flying(&self, fall_flying: bool) {
        self.set_shared_flag(EntitySharedFlags::FALL_FLYING, fall_flying);
    }

    fn set_base_on_fire_flag(&self, on_fire: bool) {
        self.set_shared_flag(EntitySharedFlags::ON_FIRE, on_fire);
    }

    fn set_base_invisible_flag(&self, invisible: bool) {
        self.set_shared_flag(EntitySharedFlags::INVISIBLE, invisible);
    }

    fn set_base_glowing_flag(&self, glowing: bool) {
        self.set_shared_flag(EntitySharedFlags::GLOWING, glowing);
    }

    fn set_base_ticks_frozen(&self, ticks_frozen: i32) {
        VanillaEntityData::base_mut(&mut *self.lock())
            .ticks_frozen
            .set(ticks_frozen);
    }
}

trait SharedFlagSetter {
    fn set_shared_flag(&self, flag: EntitySharedFlags, value: bool);
}

impl<T> SharedFlagSetter for SyncMutex<T>
where
    T: VanillaEntityData + Send + Sync,
{
    fn set_shared_flag(&self, flag: EntitySharedFlags, value: bool) {
        let mut entity_data = self.lock();
        let base = VanillaEntityData::base_mut(&mut *entity_data);
        let mut flags = EntitySharedFlags::from_metadata_byte(*base.shared_flags.get());
        flags.set(flag, value);
        base.shared_flags.set(flags.metadata_byte());
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{entity_data::EntityData, vanilla_entity_data::ItemEntityData};
    use text_components::TextComponent;

    use super::*;

    #[test]
    fn synced_data_reads_no_gravity_from_generated_base_layer() {
        let data = SyncMutex::new(ItemEntityData::new());
        assert!(!EntitySyncedData::is_no_gravity(&data));

        EntitySyncedData::set_no_gravity(&data, true);

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

    #[test]
    fn synced_data_reads_invisible_from_generated_base_layer() {
        let data = SyncMutex::new(ItemEntityData::new());
        assert!(!EntitySyncedData::is_base_invisible_flag(&data));

        EntitySyncedData::set_base_invisible_flag(&data, true);

        assert!(EntitySyncedData::is_base_invisible_flag(&data));
    }

    #[test]
    fn synced_data_writes_individual_shared_flags_without_stomping() {
        let data = SyncMutex::new(ItemEntityData::new());

        EntitySyncedData::set_shift_key_down(&data, true);
        EntitySyncedData::set_swimming(&data, true);
        EntitySyncedData::set_sprinting(&data, true);
        EntitySyncedData::set_fall_flying(&data, true);

        let flags = EntitySharedFlags::from_metadata_byte(*data.lock().base().shared_flags.get());
        assert!(flags.contains(EntitySharedFlags::SHIFT_KEY_DOWN));
        assert!(flags.contains(EntitySharedFlags::SWIMMING));
        assert!(flags.contains(EntitySharedFlags::SPRINTING));
        assert!(flags.contains(EntitySharedFlags::FALL_FLYING));

        EntitySyncedData::set_swimming(&data, false);

        let flags = EntitySharedFlags::from_metadata_byte(*data.lock().base().shared_flags.get());
        assert!(flags.contains(EntitySharedFlags::SHIFT_KEY_DOWN));
        assert!(!flags.contains(EntitySharedFlags::SWIMMING));
        assert!(flags.contains(EntitySharedFlags::SPRINTING));
        assert!(flags.contains(EntitySharedFlags::FALL_FLYING));
    }

    #[test]
    fn synced_data_writes_fire_and_freeze_base_layer() {
        let data = SyncMutex::new(ItemEntityData::new());

        data.set_base_on_fire_flag(true);
        data.set_base_ticks_frozen(12);

        let values =
            EntitySyncedData::pack_dirty(&data).expect("expected dirty base fire/freeze metadata");
        assert_eq!(values.len(), 2);
        assert!(matches!(values[0].value, EntityData::Byte(1)));
        assert!(matches!(values[1].value, EntityData::Int(12)));

        assert!(EntitySyncedData::pack_dirty(&data).is_none());
    }

    #[test]
    fn synced_data_writes_shared_save_base_layer() {
        let data = SyncMutex::new(ItemEntityData::new());

        data.set_air_supply(42);
        data.set_custom_name(Some(TextComponent::plain("Steel")));
        data.set_custom_name_visible(true);
        data.set_silent(true);
        data.set_base_glowing_flag(true);

        let values =
            EntitySyncedData::pack_dirty(&data).expect("expected dirty shared save metadata");

        assert_eq!(values.len(), 5);
        assert_eq!(values[0].index, 0);
        assert!(matches!(
            values[0].value,
            EntityData::Byte(value)
                if EntitySharedFlags::from_metadata_byte(value)
                    .contains(EntitySharedFlags::GLOWING)
        ));
        assert_eq!(values[1].index, 1);
        assert!(matches!(values[1].value, EntityData::Int(42)));
        assert_eq!(values[2].index, 2);
        assert!(matches!(
            values[2].value,
            EntityData::OptionalComponent(Some(_))
        ));
        assert_eq!(values[3].index, 3);
        assert!(matches!(values[3].value, EntityData::Boolean(true)));
        assert_eq!(values[4].index, 4);
        assert!(matches!(values[4].value, EntityData::Boolean(true)));
    }
}
