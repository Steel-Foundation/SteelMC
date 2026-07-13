use steel_registry::{
    entity_data::{DataValue, EntityPose},
    vanilla_entity_data::{
        BlockDisplayEntityData, EndCrystalEntityData, EnderPearlEntityData,
        ExperienceOrbEntityData, ItemEntityData, ItemFrameEntityData, LivingEntityData,
        PigEntityData, PlayerEntityData, VanillaEntityData,
    },
};
use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use crate::entity::EntitySharedFlags;

/// Access to an entity's vanilla synchronized data.
///
/// Read and pack methods take `&self` (the dirty state is tracked atomically), while
/// setters take `&mut self`. Entities expose the read path through
/// [`Entity::synced_data`](crate::entity::Entity::synced_data) and the write path
/// through [`Entity::synced_data_mut`](crate::entity::Entity::synced_data_mut).
pub trait EntitySyncedData: Send {
    /// Packs dirty values for network sync, clearing dirty flags.
    fn pack_dirty(&self) -> Option<Vec<DataValue>>;

    /// Packs all non-default values for initial entity spawn.
    fn pack_all(&self) -> Vec<DataValue>;

    /// Returns the shared vanilla `NoGravity` flag.
    fn is_no_gravity(&self) -> bool;

    /// Sets synchronized vanilla air supply.
    fn set_air_supply(&mut self, air_supply: i32);

    /// Sets synchronized vanilla custom name.
    fn set_custom_name(&mut self, custom_name: Option<TextComponent>);

    /// Sets synchronized vanilla custom-name visibility.
    fn set_custom_name_visible(&mut self, visible: bool);

    /// Sets synchronized vanilla silent flag.
    fn set_silent(&mut self, silent: bool);

    /// Sets the shared vanilla `NoGravity` flag.
    fn set_no_gravity(&mut self, no_gravity: bool);

    /// Sets synchronized vanilla pose.
    fn set_pose(&mut self, pose: EntityPose);

    /// Returns the shared vanilla shift-key-down flag.
    fn is_shift_key_down(&self) -> bool;

    /// Returns the shared vanilla swimming flag.
    fn is_swimming(&self) -> bool;

    /// Returns the shared vanilla invisible flag.
    fn is_base_invisible_flag(&self) -> bool;

    /// Sets the shared vanilla shift-key-down flag.
    fn set_shift_key_down(&mut self, shift_key_down: bool);

    /// Sets the shared vanilla swimming flag.
    fn set_swimming(&mut self, swimming: bool);

    /// Sets the shared vanilla sprinting flag.
    fn set_sprinting(&mut self, sprinting: bool);

    /// Sets the shared vanilla fall-flying flag.
    fn set_fall_flying(&mut self, fall_flying: bool);

    /// Sets the shared vanilla on-fire flag.
    fn set_base_on_fire_flag(&mut self, on_fire: bool);

    /// Sets the shared vanilla invisible flag.
    fn set_base_invisible_flag(&mut self, invisible: bool);

    /// Sets the shared vanilla glowing flag.
    fn set_base_glowing_flag(&mut self, glowing: bool);

    /// Sets synchronized vanilla frozen ticks.
    fn set_base_ticks_frozen(&mut self, ticks_frozen: i32);
}

/// Implements [`EntitySyncedData`] directly on a generated entity-data struct.
///
/// Used for entities that store their data without a lock and mutate it through
/// `&mut self` (everything except mobs and players, whose `&self` trait surface
/// requires interior mutability — see the [`SyncMutex`] impl below).
macro_rules! impl_entity_synced_data {
    ($($t:ty),* $(,)?) => {$(
        impl EntitySyncedData for $t {
            fn pack_dirty(&self) -> Option<Vec<DataValue>> {
                VanillaEntityData::pack_dirty(self)
            }

            fn pack_all(&self) -> Vec<DataValue> {
                VanillaEntityData::pack_all(self)
            }

            fn is_no_gravity(&self) -> bool {
                *VanillaEntityData::base(self).no_gravity.get()
            }

            fn set_air_supply(&mut self, air_supply: i32) {
                VanillaEntityData::base_mut(self).set_air_supply(air_supply);
            }

            fn set_custom_name(&mut self, custom_name: Option<TextComponent>) {
                VanillaEntityData::base_mut(self).set_custom_name(custom_name.map(Box::new));
            }

            fn set_custom_name_visible(&mut self, visible: bool) {
                VanillaEntityData::base_mut(self).set_custom_name_visible(visible);
            }

            fn set_silent(&mut self, silent: bool) {
                VanillaEntityData::base_mut(self).set_silent(silent);
            }

            fn set_no_gravity(&mut self, no_gravity: bool) {
                VanillaEntityData::base_mut(self).set_no_gravity(no_gravity);
            }

            fn set_pose(&mut self, pose: EntityPose) {
                VanillaEntityData::base_mut(self).set_pose(pose);
            }

            fn is_shift_key_down(&self) -> bool {
                EntitySharedFlags::from_metadata_byte(
                    *VanillaEntityData::base(self).shared_flags.get(),
                )
                .contains(EntitySharedFlags::SHIFT_KEY_DOWN)
            }

            fn is_swimming(&self) -> bool {
                EntitySharedFlags::from_metadata_byte(
                    *VanillaEntityData::base(self).shared_flags.get(),
                )
                .contains(EntitySharedFlags::SWIMMING)
            }

            fn is_base_invisible_flag(&self) -> bool {
                EntitySharedFlags::from_metadata_byte(
                    *VanillaEntityData::base(self).shared_flags.get(),
                )
                .contains(EntitySharedFlags::INVISIBLE)
            }

            fn set_shift_key_down(&mut self, shift_key_down: bool) {
                self.set_shared_flag(EntitySharedFlags::SHIFT_KEY_DOWN, shift_key_down);
            }

            fn set_swimming(&mut self, swimming: bool) {
                self.set_shared_flag(EntitySharedFlags::SWIMMING, swimming);
            }

            fn set_sprinting(&mut self, sprinting: bool) {
                self.set_shared_flag(EntitySharedFlags::SPRINTING, sprinting);
            }

            fn set_fall_flying(&mut self, fall_flying: bool) {
                self.set_shared_flag(EntitySharedFlags::FALL_FLYING, fall_flying);
            }

            fn set_base_on_fire_flag(&mut self, on_fire: bool) {
                self.set_shared_flag(EntitySharedFlags::ON_FIRE, on_fire);
            }

            fn set_base_invisible_flag(&mut self, invisible: bool) {
                self.set_shared_flag(EntitySharedFlags::INVISIBLE, invisible);
            }

            fn set_base_glowing_flag(&mut self, glowing: bool) {
                self.set_shared_flag(EntitySharedFlags::GLOWING, glowing);
            }

            fn set_base_ticks_frozen(&mut self, ticks_frozen: i32) {
                VanillaEntityData::base_mut(self).set_ticks_frozen(ticks_frozen);
            }
        }
    )*};
}

impl_entity_synced_data!(
    ItemEntityData,
    EnderPearlEntityData,
    BlockDisplayEntityData,
    ExperienceOrbEntityData,
    EndCrystalEntityData,
    ItemFrameEntityData,
    PigEntityData,
    PlayerEntityData,
    LivingEntityData,
);

/// Mobs and players keep their synced data behind a [`SyncMutex`] because their
/// `&self` trait surface (`LivingEntity`, `Mob`, …) mutates through interior
/// mutability. Reads lock the mutex; writes use exclusive `&mut self` access via
/// `get_mut`, so they never block.
impl<T> EntitySyncedData for SyncMutex<T>
where
    T: EntitySyncedData,
{
    fn pack_dirty(&self) -> Option<Vec<DataValue>> {
        self.lock().pack_dirty()
    }

    fn pack_all(&self) -> Vec<DataValue> {
        self.lock().pack_all()
    }

    fn is_no_gravity(&self) -> bool {
        self.lock().is_no_gravity()
    }

    fn set_air_supply(&mut self, air_supply: i32) {
        self.get_mut().set_air_supply(air_supply);
    }

    fn set_custom_name(&mut self, custom_name: Option<TextComponent>) {
        self.get_mut().set_custom_name(custom_name);
    }

    fn set_custom_name_visible(&mut self, visible: bool) {
        self.get_mut().set_custom_name_visible(visible);
    }

    fn set_silent(&mut self, silent: bool) {
        self.get_mut().set_silent(silent);
    }

    fn set_no_gravity(&mut self, no_gravity: bool) {
        self.get_mut().set_no_gravity(no_gravity);
    }

    fn set_pose(&mut self, pose: EntityPose) {
        self.get_mut().set_pose(pose);
    }

    fn is_shift_key_down(&self) -> bool {
        self.lock().is_shift_key_down()
    }

    fn is_swimming(&self) -> bool {
        self.lock().is_swimming()
    }

    fn is_base_invisible_flag(&self) -> bool {
        self.lock().is_base_invisible_flag()
    }

    fn set_shift_key_down(&mut self, shift_key_down: bool) {
        self.get_mut().set_shift_key_down(shift_key_down);
    }

    fn set_swimming(&mut self, swimming: bool) {
        self.get_mut().set_swimming(swimming);
    }

    fn set_sprinting(&mut self, sprinting: bool) {
        self.get_mut().set_sprinting(sprinting);
    }

    fn set_fall_flying(&mut self, fall_flying: bool) {
        self.get_mut().set_fall_flying(fall_flying);
    }

    fn set_base_on_fire_flag(&mut self, on_fire: bool) {
        self.get_mut().set_base_on_fire_flag(on_fire);
    }

    fn set_base_invisible_flag(&mut self, invisible: bool) {
        self.get_mut().set_base_invisible_flag(invisible);
    }

    fn set_base_glowing_flag(&mut self, glowing: bool) {
        self.get_mut().set_base_glowing_flag(glowing);
    }

    fn set_base_ticks_frozen(&mut self, ticks_frozen: i32) {
        self.get_mut().set_base_ticks_frozen(ticks_frozen);
    }
}

trait SharedFlagSetter {
    fn set_shared_flag(&mut self, flag: EntitySharedFlags, value: bool);
}

impl<T> SharedFlagSetter for T
where
    T: VanillaEntityData + Send + Sync,
{
    fn set_shared_flag(&mut self, flag: EntitySharedFlags, value: bool) {
        let base = VanillaEntityData::base_mut(self);
        let mut flags = EntitySharedFlags::from_metadata_byte(*base.shared_flags.get());
        flags.set(flag, value);
        base.set_shared_flags(flags.metadata_byte());
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{entity_data::EntityData, vanilla_entity_data::ItemEntityData};
    use text_components::TextComponent;

    use super::*;

    #[test]
    fn synced_data_reads_no_gravity_from_generated_base_layer() {
        let mut data = ItemEntityData::new();
        assert!(!EntitySyncedData::is_no_gravity(&data));

        EntitySyncedData::set_no_gravity(&mut data, true);

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
        let mut data = ItemEntityData::new();
        assert!(!EntitySyncedData::is_shift_key_down(&data));

        data.base_mut()
            .set_shared_flags(EntitySharedFlags::SHIFT_KEY_DOWN.metadata_byte());

        assert!(EntitySyncedData::is_shift_key_down(&data));
    }

    #[test]
    fn synced_data_reads_swimming_from_generated_base_layer() {
        let mut data = ItemEntityData::new();
        assert!(!EntitySyncedData::is_swimming(&data));

        data.base_mut()
            .set_shared_flags(EntitySharedFlags::SWIMMING.metadata_byte());

        assert!(EntitySyncedData::is_swimming(&data));
    }

    #[test]
    fn synced_data_reads_invisible_from_generated_base_layer() {
        let mut data = ItemEntityData::new();
        assert!(!EntitySyncedData::is_base_invisible_flag(&data));

        EntitySyncedData::set_base_invisible_flag(&mut data, true);

        assert!(EntitySyncedData::is_base_invisible_flag(&data));
    }

    #[test]
    fn synced_data_writes_individual_shared_flags_without_stomping() {
        let mut data = ItemEntityData::new();

        EntitySyncedData::set_shift_key_down(&mut data, true);
        EntitySyncedData::set_swimming(&mut data, true);
        EntitySyncedData::set_sprinting(&mut data, true);
        EntitySyncedData::set_fall_flying(&mut data, true);

        let flags = EntitySharedFlags::from_metadata_byte(*data.base().shared_flags.get());
        assert!(flags.contains(EntitySharedFlags::SHIFT_KEY_DOWN));
        assert!(flags.contains(EntitySharedFlags::SWIMMING));
        assert!(flags.contains(EntitySharedFlags::SPRINTING));
        assert!(flags.contains(EntitySharedFlags::FALL_FLYING));

        EntitySyncedData::set_swimming(&mut data, false);

        let flags = EntitySharedFlags::from_metadata_byte(*data.base().shared_flags.get());
        assert!(flags.contains(EntitySharedFlags::SHIFT_KEY_DOWN));
        assert!(!flags.contains(EntitySharedFlags::SWIMMING));
        assert!(flags.contains(EntitySharedFlags::SPRINTING));
        assert!(flags.contains(EntitySharedFlags::FALL_FLYING));
    }

    #[test]
    fn synced_data_writes_fire_and_freeze_base_layer() {
        let mut data = ItemEntityData::new();

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
        let mut data = ItemEntityData::new();

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
