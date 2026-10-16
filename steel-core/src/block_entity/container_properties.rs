use simdnbt::{
    FromNbtTag as _,
    borrow::{BaseNbtCompound, NbtCompound as BorrowedNbtCompound},
    owned::NbtCompound,
};
use std::sync::Arc;
use steel_registry::{
    data_components::vanilla_components::{CUSTOM_NAME, LOCK},
    equipment::EquipmentSlot,
    item_predicate::LockCode,
    item_stack::ItemStack,
};
use steel_utils::locks::SyncRwLock;
use text_components::TextComponent;

use crate::{
    entity::{Entity as _, LivingEntity as _},
    item_predicate::registered_item_predicate_matches,
    player::Player,
};

struct Properties {
    name: Option<TextComponent>,
    lock: LockCode,
}

impl Default for Properties {
    fn default() -> Self {
        Self {
            name: None,
            lock: LockCode::NO_LOCK,
        }
    }
}

/// Name and lock state shared by block-container owners.
#[derive(Default)]
pub(crate) struct ContainerProperties {
    properties: SyncRwLock<Arc<Properties>>,
}

impl ContainerProperties {
    pub(crate) fn load(&self, nbt: &BaseNbtCompound<'_>) {
        let nbt: BorrowedNbtCompound<'_, '_> = nbt.into();
        let name = nbt.get("CustomName").and_then(|tag| {
            let name = TextComponent::from_nbt(&tag.to_owned());
            if name.is_none() {
                log::warn!("Invalid container CustomName");
            }
            name
        });
        let lock = nbt
            .get("lock")
            .and_then(|tag| {
                let lock = LockCode::from_nbt_tag(tag);
                if lock.is_none() {
                    log::warn!("Invalid container lock predicate");
                }
                lock
            })
            .unwrap_or(LockCode::NO_LOCK);
        *self.properties.write() = Arc::new(Properties { name, lock });
    }

    pub(crate) fn save(&self, nbt: &mut NbtCompound) {
        let properties = self.properties.read();
        if let Some(name) = &properties.name {
            nbt.insert("CustomName", name.to_codec_nbt());
        }
        if properties.lock != LockCode::NO_LOCK {
            nbt.insert("lock", properties.lock.to_nbt_tag_ref());
        }
    }

    pub(crate) fn apply_components(&self, item: &ItemStack) {
        *self.properties.write() = Arc::new(Properties {
            name: item.get(CUSTOM_NAME).cloned(),
            lock: item.get(LOCK).cloned().unwrap_or(LockCode::NO_LOCK),
        });
    }

    pub(crate) fn custom_name(&self) -> Option<TextComponent> {
        self.properties.read().name.clone()
    }

    pub(crate) fn can_open(&self, player: &Player) -> bool {
        if player.is_spectator() {
            return true;
        }
        let properties = Arc::clone(&self.properties.read());
        let mut can_open = false;
        player.with_equipment_slot(EquipmentSlot::MainHand, &mut |item| {
            can_open = registered_item_predicate_matches(properties.lock.predicate(), item);
        });
        can_open
    }
}
