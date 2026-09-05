//! Leash fence knot entity foundation.

use std::sync::{Arc, Weak};

use crate::behavior::InteractionResult;
use crate::entity::block_attached_entity::{BlockAttachedEntity, BlockAttachedEntityBase};
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityMoveError, RemovalReason,
    SharedEntity, next_entity_id,
};
use crate::physics::{MoveResult, MoverType};
use crate::player::Player;
use crate::world::World;
use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events::ITEM_LEAD_TIED;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_game_events::BLOCK_ATTACH;
use steel_registry::{sound_events, vanilla_items};
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Downcast as _, DowncastType, DowncastTypeKey, WorldAabb};

/// Vanilla leash knot attached to a fence block.
#[entity_behavior(class = "LeashFenceKnotEntity")]
pub struct LeashFenceKnotEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    block_attached_entity_base: BlockAttachedEntityBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `LeashFenceKnotEntity`.
unsafe impl DowncastType for LeashFenceKnotEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/leash_fence_knot");
}

impl LeashFenceKnotEntity {
    /// Creates a fresh leash knot from the generic entity factory path.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_attached(
            entity_type,
            id,
            BlockPos::new(
                position.x.floor() as i32,
                position.y.floor() as i32,
                position.z.floor() as i32,
            ),
            world,
        )
    }

    /// Creates a fresh leash knot attached to `block_pos`.
    #[must_use]
    pub fn new_attached(
        entity_type: EntityTypeRef,
        id: i32,
        block_pos: BlockPos,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::new_with_state(
                id,
                EntityBaseState::new_with_bounding_box(
                    Self::knot_center(block_pos),
                    entity_type.dimensions,
                    Self::knot_bounding_box(entity_type, block_pos),
                ),
                world,
            ),
            entity_type,
            block_attached_entity_base: BlockAttachedEntityBase::new(block_pos),
        }
    }

    /// Creates a leash knot from persistent entity data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let position = load.position;
        let block_pos = BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            block_attached_entity_base: BlockAttachedEntityBase::new(block_pos),
        }
    }

    /// Returns the fence block this knot is attached to.
    #[must_use]
    pub fn block_pos(&self) -> BlockPos {
        self.block_attached_entity_base.pos()
    }

    /// Finds an existing leash knot at `pos`.
    #[must_use]
    pub fn get_knot(world: &World, pos: BlockPos) -> Option<SharedEntity> {
        let search_box = WorldAabb::new(
            f64::from(pos.x()) - 1.0,
            f64::from(pos.y()) - 1.0,
            f64::from(pos.z()) - 1.0,
            f64::from(pos.x()) + 1.0,
            f64::from(pos.y()) + 1.0,
            f64::from(pos.z()) + 1.0,
        );
        world
            .get_entities_in_aabb_matching(&search_box, |entity| {
                entity
                    .downcast_ref::<Self>()
                    .is_some_and(|knot| knot.block_pos() == pos)
            })
            .into_iter()
            .next()
    }

    /// Gets or creates a leash knot at `pos`.
    #[must_use]
    pub fn get_or_create_knot(world: &Arc<World>, pos: BlockPos) -> Option<SharedEntity> {
        if let Some(knot) = Self::get_knot(world.as_ref(), pos) {
            return Some(knot);
        }

        let knot: SharedEntity = Arc::new(Self::new_attached(
            &vanilla_entities::LEASH_KNOT,
            next_entity_id(),
            pos,
            Arc::downgrade(world),
        ));
        if let Err(error) = world.try_add_entity(Arc::clone(&knot)) {
            log::warn!("Failed to spawn leash knot entity: {error}");
            return None;
        }

        Some(knot)
    }

    fn knot_center(block_pos: BlockPos) -> DVec3 {
        DVec3::new(
            f64::from(block_pos.x()) + 0.5,
            f64::from(block_pos.y()) + 0.375,
            f64::from(block_pos.z()) + 0.5,
        )
    }

    fn knot_bounding_box(entity_type: EntityTypeRef, block_pos: BlockPos) -> WorldAabb {
        let center = Self::knot_center(block_pos);
        let half_width = f64::from(entity_type.dimensions.width) / 2.0;
        let height = f64::from(entity_type.dimensions.height);
        WorldAabb::new(
            center.x - half_width,
            center.y,
            center.z - half_width,
            center.x + half_width,
            center.y + height,
            center.z + half_width,
        )
    }
}

impl Entity for LeashFenceKnotEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn spawn_position(&self) -> DVec3 {
        let block_pos = self.block_pos();
        DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        )
    }

    fn notify_leashee_removed(&self, _leashable: &dyn Entity) {
        if self.level().is_some() && self.leashables_leashed_to().is_empty() {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };

        let holding_shears = {
            let inventory = player.inventory.lock();
            inventory.get_item_in_hand(hand).is(&vanilla_items::SHEARS)
        };
        if holding_shears {
            let result = self.interact_entity(player, hand, location);
            if result == InteractionResult::Success {
                return result;
            }
        }

        let mut attached_mob = false;
        let Some(knot) = world.get_entity_by_id(self.id()) else {
            return InteractionResult::Pass;
        };
        for entity in player.leashables_leashed_to() {
            if let Some(leashable) = entity.as_leashable()
                && leashable.can_have_a_leash_attached_to(self)
            {
                leashable.set_leashed_to(&knot);
                attached_mob = true;
            }
        }

        let mut any_dropped = false;
        let Some(player_entity) = world.get_entity_by_id(player.id()) else {
            return InteractionResult::Pass;
        };
        if !attached_mob && !player.is_secondary_use_active() {
            for entity in knot.leashables_leashed_to() {
                if let Some(leashable) = entity.as_leashable()
                    && leashable.can_have_a_leash_attached_to(player)
                {
                    leashable.set_leashed_to(&player_entity);
                    any_dropped = true;
                }
            }
        }

        if !attached_mob && !any_dropped {
            return self.interact_entity(player, hand, location);
        }

        self.game_event_with_source_entity(&BLOCK_ATTACH, Some(player));
        self.play_sound(&ITEM_LEAD_TIED, 1.0, 1.0);

        InteractionResult::Success
    }

    fn tick(&self) {
        self.tick_block_attached_entity();
    }

    fn is_pickable(&self) -> bool {
        self.is_pickable_block_attached_entity()
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.hurt_block_attached_entity(world, source, amount)
    }

    fn skip_attack_interaction(&self, source: &dyn Entity) -> bool {
        self.skip_attack_interaction_block_attached_entity(source)
    }

    fn refresh_dimensions(&self) {
        self.refresh_dimensions_block_attached_entity();
    }

    fn push_impulse(&self, impulse: DVec3) {
        self.push_impulse_block_attached_entity(impulse);
    }

    fn move_entity(&self, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        self.move_entity_block_attached_entity(mover_type, delta)
    }

    fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position_block_attached_entity(pos)
    }
}

impl BlockAttachedEntity for LeashFenceKnotEntity {
    fn block_attached_entity_base(&self) -> &BlockAttachedEntityBase {
        &self.block_attached_entity_base
    }

    fn survives(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        world
            .get_block_state(self.block_pos())
            .get_block()
            .has_tag(&BlockTag::FENCES)
    }

    fn drop_item(&self, _caused_by: Option<&dyn Entity>) {
        self.play_sound(&sound_events::ITEM_LEAD_UNTIED, 1.0, 1.0);

        // Vanilla does not drop a lead here. However, due to how Rust handles `Weak`
        // pointers to leash holders when a holder despawns, in the code where a lead
        // is supposed in drop in Vanilla, the holder is `None`. So, a lead does not drop
        // in `leash_tick`. We can replicate this behavior by dropping it for each entity instead.
        for entity in self.leashables_leashed_to() {
            entity.spawn_at_location(ItemStack::new(&vanilla_items::LEAD), 0.0);
        }
    }

    fn recalculate_bounding_box(&self) -> Result<(), EntityMoveError> {
        let pos = self.block_attached_entity_base.pos();
        self.try_set_position_base(Self::knot_center(pos))?;
        self.base()
            .set_bounding_box(Self::knot_bounding_box(self.entity_type, pos));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simdnbt::owned::NbtCompound;

    #[test]
    fn leash_knot_uses_vanilla_position_and_bounding_box() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        assert_eq!(knot.position(), DVec3::new(4.5, 65.375, -8.5));
        assert_eq!(
            knot.bounding_box(),
            LeashFenceKnotEntity::knot_bounding_box(
                &vanilla_entities::LEASH_KNOT,
                BlockPos::new(4, 65, -9)
            )
        );
    }

    #[test]
    fn leash_knot_spawn_packet_uses_attached_block_pos() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        assert_eq!(knot.spawn_position(), DVec3::new(4.0, 65.0, -9.0));
    }

    #[test]
    fn leash_knot_saves_no_type_specific_block_pos() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        let mut nbt = NbtCompound::new();
        knot.save_additional(&mut nbt);

        assert!(nbt.is_empty());
    }

    #[test]
    fn leash_knot_survival_check_matches_vanilla_interval() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        for _ in 0..100 {
            assert!(!knot.block_attached_entity_base().should_check_survival());
        }
        assert!(knot.block_attached_entity_base().should_check_survival());
        assert!(!knot.block_attached_entity_base().should_check_survival());
    }
}
