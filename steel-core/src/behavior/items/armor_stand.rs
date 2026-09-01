//! Armor stand item behavior (`ArmorStandItem`).

use std::io::Cursor;
use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::properties::Direction;
use steel_registry::data_components::vanilla_components::{CUSTOM_NAME, ENTITY_DATA};
use steel_registry::{sound_events, vanilla_entities, vanilla_game_events};
use steel_utils::{WorldAabb, wrap_degrees};

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::{ENTITIES, Entity as _, next_entity_id};
use crate::physics::{WorldCollisionProvider, has_collision};
use crate::world::game_event::GameEventContext;

/// Behavior for placing armor stands.
#[item_behavior(class = "ArmorStandItem")]
#[derive(Default)]
pub struct ArmorStandItem;

impl ArmorStandItem {
    /// Creates a new `ArmorStandItem` behavior.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ItemBehavior for ArmorStandItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let clicked_face = context.hit_result.direction;
        if clicked_face == Direction::Down {
            return InteractionResult::Fail;
        }

        let place_context = context.build_place_context();
        let block_pos = place_context.place_pos();
        let pos = DVec3::new(
            f64::from(block_pos.x()) + 0.5,
            f64::from(block_pos.y()),
            f64::from(block_pos.z()) + 0.5,
        );
        let dimensions = vanilla_entities::ARMOR_STAND.dimensions;
        let box_aabb = WorldAabb::entity_box(
            pos.x,
            pos.y,
            pos.z,
            f64::from(dimensions.width) * 0.5,
            f64::from(dimensions.height),
        );

        let collision_provider = WorldCollisionProvider::new(context.world);
        if !has_collision(&collision_provider, box_aabb)
            && context.world.get_entities_in_aabb(&box_aabb).is_empty()
        {
            let y_rot = (((wrap_degrees(context.player.rotation().0 - 180.0) + 22.5) / 45.0)
                .floor()
                * 45.0) as f32;

            let entity_id = next_entity_id();
            let Some(entity) = ENTITIES.create(
                &vanilla_entities::ARMOR_STAND,
                entity_id,
                pos,
                Arc::downgrade(context.world),
            ) else {
                return InteractionResult::Fail;
            };

            context.inv.with_item(|stack| {
                if let Some(custom_name) = stack.get(CUSTOM_NAME) {
                    entity.set_custom_name(Some(custom_name.clone()));
                }
                if let Some(entity_data) = stack.get(ENTITY_DATA) {
                    let mut bytes = Vec::new();
                    entity_data.data().as_compound().write(&mut bytes);
                    if let Ok(borrowed) = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
                    {
                        entity.load_additional((&borrowed).into());
                    }
                }
            });

            entity.set_rotation((y_rot, 0.0));
            if let Some(living) = entity.as_living_entity() {
                living.set_y_body_rot(y_rot);
                living.set_y_head_rot(y_rot);
            }

            if context.world.try_add_entity(entity).is_err() {
                return InteractionResult::Fail;
            }

            context.world.play_sound_at(
                &sound_events::ENTITY_ARMOR_STAND_PLACE,
                SoundSource::Blocks,
                pos,
                0.75,
                0.8,
                None,
            );

            context.world.game_event(
                &vanilla_game_events::ENTITY_PLACE,
                block_pos,
                &GameEventContext::new(Some(context.player), None),
            );

            if !context.player.has_infinite_materials() {
                context.inv.with_item(|item| item.shrink(1));
            }

            InteractionResult::Success
        } else {
            InteractionResult::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Weak;

    use glam::DVec3;
    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_registry::data_components::components::{CustomData, EntityData};
    use steel_registry::data_components::vanilla_components::ENTITY_DATA;
    use steel_registry::entity_data::Rotations;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_entities, vanilla_items};
    use steel_utils::wrap_degrees;

    use crate::entity::Entity as _;
    use crate::entity::entities::ArmorStandEntity;

    #[test]
    fn armor_stand_yaw_snaps_to_45_degrees() {
        let player_yaw = 30.0;
        let y_rot = (((wrap_degrees(player_yaw - 180.0) + 22.5) / 45.0).floor() * 45.0) as f32;
        assert_eq!(y_rot % 45.0, 0.0);

        let player_yaw_2 = 180.0;
        let y_rot_2 = (((wrap_degrees(player_yaw_2 - 180.0) + 22.5) / 45.0).floor() * 45.0) as f32;
        assert_eq!(y_rot_2, 0.0);
    }

    #[test]
    fn armor_stand_item_entity_data_applied() {
        init_vanilla_registry();

        let mut item = ItemStack::new(&vanilla_items::ARMOR_STAND);
        let mut custom_nbt = NbtCompound::new();
        custom_nbt.insert("ShowArms", 1i8);

        let mut pose_compound = NbtCompound::new();
        pose_compound.insert(
            "LeftArm",
            NbtTag::List(NbtList::Float(vec![270.0, 0.0, 0.0])),
        );
        pose_compound.insert(
            "RightArm",
            NbtTag::List(NbtList::Float(vec![270.0, 0.0, 0.0])),
        );
        custom_nbt.insert("Pose", pose_compound);

        let custom_data =
            CustomData::try_from_compound(custom_nbt).expect("valid custom nbt compound");
        let entity_data = EntityData::new(&vanilla_entities::ARMOR_STAND, custom_data);
        item.set(ENTITY_DATA, entity_data);

        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        if let Some(ed) = item.get(ENTITY_DATA) {
            let mut bytes = Vec::new();
            ed.data().as_compound().write(&mut bytes);
            let borrowed =
                read_borrowed_compound(&mut Cursor::new(&bytes)).expect("valid compound");
            armor_stand.load_additional((&borrowed).into());
        }

        assert!(armor_stand.show_arms());
        assert_eq!(armor_stand.left_arm_pose(), Rotations::new(270.0, 0.0, 0.0));
        assert_eq!(
            armor_stand.right_arm_pose(),
            Rotations::new(270.0, 0.0, 0.0)
        );
    }
}
