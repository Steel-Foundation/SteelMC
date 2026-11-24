//! Owned container-event inputs retained during exploration-map chunk requests.

use super::{EntityEquipmentRef, EntityEquipmentSlots, EntityRef, EntityRefFlags, LootContext};
use crate::{
    entity::{Entity, entity_loot_ref, loot_equipment},
    world::World,
};
use glam::DVec3;
use std::sync::Arc;
use steel_registry::{DyeColor, entity_variant::MooshroomVariant, equipment::EquipmentSlot};
use steel_utils::{Identifier, random::Random};

/// Loot-visible entity values captured at the operation's triggering event.
pub struct LootEntitySnapshot {
    entity_type: Option<Identifier>,
    flags: EntityRefFlags,
    equipment: Option<EntityEquipmentSlots>,
    custom_name: Option<String>,
    sheep_color: Option<DyeColor>,
    sheep_sheared: Option<bool>,
    fishing_hook_open_water: Option<bool>,
    chicken_variant: Option<Identifier>,
    mooshroom_variant: Option<MooshroomVariant>,
}

impl LootEntitySnapshot {
    /// Copies all supported entity inputs, including equipment used by looting.
    pub fn capture(entity: &dyn Entity) -> Self {
        Self::from_ref(entity_loot_ref(entity, None))
    }

    /// Captures an existing event view, materializing live equipment if necessary.
    pub fn from_ref(entity: EntityRef<'_>) -> Self {
        let equipment = entity
            .equipment
            .map(|equipment| {
                EquipmentSlot::ALL.map(|slot| {
                    let item = equipment.get(slot);
                    item.copy_with_count(item.count())
                })
            })
            .or_else(|| entity.entity.and_then(loot_equipment));
        Self {
            entity_type: entity.entity_type.cloned(),
            flags: entity.flags,
            equipment,
            custom_name: entity.custom_name.map(str::to_owned),
            sheep_color: entity.sheep_color,
            sheep_sheared: entity.sheep_sheared,
            fishing_hook_open_water: entity.fishing_hook_open_water,
            chicken_variant: entity.chicken_variant.cloned(),
            mooshroom_variant: entity.mooshroom_variant,
        }
    }

    fn as_ref(&self) -> EntityRef<'_> {
        EntityRef {
            entity: None,
            entity_type: self.entity_type.as_ref(),
            flags: self.flags,
            equipment: self.equipment.as_ref().map(EntityEquipmentRef::new),
            custom_name: self.custom_name.as_deref(),
            sheep_color: self.sheep_color,
            sheep_sheared: self.sheep_sheared,
            fishing_hook_open_water: self.fishing_hook_open_water,
            chicken_variant: self.chicken_variant.as_ref(),
            mooshroom_variant: self.mooshroom_variant,
        }
    }
}

/// Container parameters captured when access first requests lazy loot.
#[derive(Default)]
pub struct LootParams {
    /// World containing the container.
    pub world: Option<Arc<World>>,
    /// Opening player's luck, or zero for non-player access.
    pub luck: f32,
    /// Container center.
    pub origin: Option<DVec3>,
    /// Opening player, when access was initiated by one.
    pub this_entity: Option<LootEntitySnapshot>,
}

impl LootParams {
    /// Borrows the container snapshot using its reserved random stream.
    pub fn context<'a>(&'a self, random: &'a mut (dyn Random + Send)) -> LootContext<'a> {
        let mut context = LootContext::new(random);
        context.world = self.world.as_deref();
        context.luck = self.luck;
        context.origin = self.origin;
        context.this_entity = self.this_entity.as_ref().map(LootEntitySnapshot::as_ref);
        context
    }
}
