use super::{ItemFrameEntity, direction_3d_data_value};
use crate::{
    behavior::InteractionResult,
    entity::{
        BlockAttachedEntity, Entity, EntityBase, EntityMoveError, EntitySyncedData, HangingEntity,
        ItemFrame, SharedEntity, damage::DamageSource, entities::ItemEntity,
    },
    maps::MapItemSavedData,
    physics::{MoveResult, MoverType},
    player::Player,
    world::{Explosion, World},
};
use glam::DVec3;
use simdnbt::{borrow::NbtCompound as BorrowedNbtCompoundView, owned::NbtCompound};
use std::sync::Arc;
use steel_registry::{
    data_components::vanilla_components::MAP_ID, entity_type::EntityTypeRef, item_stack::ItemStack,
    sound_events, vanilla_damage_type_tags::DamageTypeTag, vanilla_entities, vanilla_game_events,
};
use steel_utils::{
    Direction,
    nbt::{NbtNumeric, NbtValueInput},
    types::InteractionHand,
};

impl Entity for ItemFrameEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn spawn_data(&self) -> i32 {
        direction_3d_data_value(*self.entity_data.lock().hanging_entity.direction.get())
    }

    fn spawn_position(&self) -> DVec3 {
        let block_pos = self.block_attached_entity_base.pos();
        DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        )
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn tick(self: Arc<Self>) {
        self.tick_block_attached_entity();
    }

    fn is_pickable(&self) -> bool {
        self.is_pickable_block_attached_entity()
    }

    fn ignore_explosion(&self, explosion: &dyn Explosion) -> bool {
        explosion
            .direct_source_entity()
            .is_some_and(Entity::is_in_water)
            || !explosion.should_affect_blocklike_entities()
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.frame_state.lock().fixed {
            return (source.bypasses_invulnerability() || source.is_creative_player())
                && self.hurt_block_attached_entity(world, source, amount);
        }
        if self.is_invulnerable_to_base(source) {
            return false;
        }
        if !source.is(&DamageTypeTag::IS_EXPLOSION) && !self.item().is_empty() {
            self.drop_contents(source.causing_entity().map(AsRef::as_ref), false);
            self.game_event_with_source_entity(
                &vanilla_game_events::BLOCK_CHANGE,
                source.causing_entity().map(AsRef::as_ref),
            );
            self.play_sound(
                if self.entity_type == &vanilla_entities::GLOW_ITEM_FRAME {
                    &sound_events::ENTITY_GLOW_ITEM_FRAME_REMOVE_ITEM
                } else {
                    &sound_events::ENTITY_ITEM_FRAME_REMOVE_ITEM
                },
                1.0,
                1.0,
            );
            return true;
        }
        self.hurt_block_attached_entity(world, source, amount)
    }

    fn kill(&self, _world: &World) {
        self.kill_attributed(None);
    }

    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        if self.frame_state.lock().fixed {
            return InteractionResult::Pass;
        }
        if self.item().is_empty() {
            let item = player.inventory.lock().get_item_in_hand(hand).clone();
            if item.is_empty() || self.is_removed() {
                return InteractionResult::Pass;
            }
            if let Some(world) = self.level()
                && let Some(id) = item.get(MAP_ID)
                && let Some(data) = world.get_map_data(*id)
                && data
                    .lock()
                    .is_tracked_count_over_limit(MapItemSavedData::TRACKED_DECORATION_LIMIT)
            {
                return InteractionResult::Fail;
            }
            self.set_item(item);
            self.game_event_with_source_entity(&vanilla_game_events::BLOCK_CHANGE, Some(player));
            if !player.has_infinite_materials() {
                player.inventory.lock().shrink_item_in_hand(hand, 1);
            }
        } else {
            self.play_sound(
                if self.entity_type == &vanilla_entities::GLOW_ITEM_FRAME {
                    &sound_events::ENTITY_GLOW_ITEM_FRAME_ROTATE_ITEM
                } else {
                    &sound_events::ENTITY_ITEM_FRAME_ROTATE_ITEM
                },
                1.0,
                1.0,
            );
            let rotation = *self.entity_data.lock().rotation.get();
            self.set_rotation(rotation.wrapping_add(1));
            self.game_event_with_source_entity(&vanilla_game_events::BLOCK_CHANGE, Some(player));
        }
        InteractionResult::Success
    }

    fn spawn_at_location(&self, item: ItemStack, y_offset: f64) -> Option<Arc<ItemEntity>> {
        let direction = self.direction().offset_vec().as_dvec3();
        self.spawn_at_location_with_offset(
            item,
            DVec3::new(
                direction.x * f64::from(0.15_f32),
                y_offset,
                direction.z * f64::from(0.15_f32),
            ),
        )
    }

    fn skip_attack_interaction(&self, source: &SharedEntity) -> bool {
        self.skip_attack_interaction_block_attached_entity(source)
    }

    fn push_impulse(&self, impulse: DVec3) {
        if !self.frame_state.lock().fixed {
            self.push_impulse_block_attached_entity(impulse);
        }
    }

    fn move_entity(self: Arc<Self>, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        if self.frame_state.lock().fixed {
            return None;
        }
        self.move_entity_block_attached_entity(mover_type, delta)
    }

    fn refresh_dimensions(&self) {
        self.refresh_dimensions_block_attached_entity();
    }

    fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position_block_attached_entity(pos)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_block_attached_entity(nbt);

        let entity_data = self.entity_data.lock();
        let item = entity_data.item.get();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag_ref());
        }
        nbt.insert("ItemRotation", *entity_data.rotation.get() as i8);
        nbt.insert("ItemDropChance", self.frame_state.lock().drop_chance);
        nbt.insert(
            "Facing",
            direction_3d_data_value(*entity_data.hanging_entity.direction.get()) as i8,
        );
        drop(entity_data);
        nbt.insert("Invisible", i8::from(self.is_invisible()));
        nbt.insert("Fixed", i8::from(self.frame_state.lock().fixed));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_block_attached_entity(nbt);

        let item = nbt
            .compound("Item")
            .and_then(|tag| ItemStack::from_borrowed_compound(&tag))
            .unwrap_or_else(ItemStack::empty);
        let previous = self.item();
        if !previous.is_empty() && !ItemStack::matches(&item, &previous) {
            self.remove_framed_map(&previous);
        }
        self.set_item_with_update(item, false);
        self.entity_data
            .lock()
            .rotation
            .set(i32::from(nbt.byte_value("ItemRotation").unwrap_or(0)) % 8);
        self.frame_state.lock().drop_chance = nbt
            .get("ItemDropChance")
            .and_then(|value| value.codec_f32())
            .unwrap_or(1.0);
        self.frame_state.lock().fixed = nbt.byte_value("Fixed").is_some_and(|value| value != 0);
        self.entity_data
            .set_base_invisible_flag(nbt.byte_value("Invisible").is_some_and(|value| value != 0));

        let facing = nbt
            .get("Facing")
            .and_then(|tag| tag.codec_i8())
            .map_or(Direction::Down, |value| {
                Direction::from_3d_data_value(i32::from(value))
            });
        self.set_direction(facing);

        self.recalculate_position_or_warn();
    }
}
