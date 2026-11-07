use std::io::Cursor;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use steel_registry::{
    sound_events, vanilla_attributes, vanilla_cow_sound_variants, vanilla_cow_variants,
    vanilla_damage_types, vanilla_entities, vanilla_items,
};
use steel_utils::types::InteractionHand;

use crate::behavior::InteractionResult;
use crate::entity::damage::DamageSource;
use crate::entity::{EntitySpawnReason, MobParts as _};
use crate::test_support::{TestPlayerBuilder, fresh_test_world};

use super::*;

mod core;
mod persistence;
