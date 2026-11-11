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
};
use steel_utils::locks::SyncRwLock;
use text_components::TextComponent;

use crate::{
    block_entity::ImplicitComponentGetter,
    entity::{Entity as _, LivingEntity as _},
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

    pub(crate) fn apply_components(&self, components: &mut ImplicitComponentGetter<'_>) {
        *self.properties.write() = Arc::new(Properties {
            name: components.get(CUSTOM_NAME),
            lock: components.get_or_default(LOCK, LockCode::NO_LOCK),
        });
    }

    pub(crate) fn custom_name(&self) -> Option<TextComponent> {
        self.properties.read().name.clone()
    }

    pub(crate) fn can_open(&self, player: &Player) -> bool {
        if player.is_spectator() {
            return true;
        }
        let properties = {
            let properties = self.properties.read();
            if properties.lock == LockCode::NO_LOCK {
                return true;
            }
            Arc::clone(&properties)
        };
        let mut can_open = false;
        player.with_equipment_slot(EquipmentSlot::MainHand, &mut |item| {
            can_open = properties.lock.predicate().matches(item);
        });
        can_open
    }
}
