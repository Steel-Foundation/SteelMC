use std::io::Cursor;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use steel_registry::RegistryExt as _;
use steel_registry::{
    sound_events, vanilla_attributes, vanilla_chicken_sound_variants, vanilla_chicken_variants,
    vanilla_damage_types, vanilla_entities, vanilla_items,
};
use steel_utils::Downcast as _;

use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntitySpawnReason};
use crate::test_support::fresh_test_world;

use super::*;

mod core;
mod persistence;
