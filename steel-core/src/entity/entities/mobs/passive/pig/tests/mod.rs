use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::owned::NbtCompound;
use steel_registry::init_vanilla_registry;
use steel_registry::{
    vanilla_entities, vanilla_items, vanilla_pig_sound_variants, vanilla_pig_variants,
};
use steel_utils::Downcast as _;
use steel_utils::types::InteractionHand;

use crate::bootstrap::init_globals;
use crate::entity::EntityBase;
use crate::player::ResetReason;
use crate::test_support::{TestPlayerBuilder, fresh_test_world};

use super::*;

mod core;
mod riding;

fn pig(id: i32) -> Arc<PigEntity> {
    PigEntity::new(&vanilla_entities::PIG, id, DVec3::ZERO, Weak::new())
}

fn saddle(pig: &PigEntity) {
    pig.living()
        .with_equipment_slot_mut(EquipmentSlot::Saddle, &mut |stack| {
            *stack = ItemStack::new(&vanilla_items::SADDLE);
        });
}
