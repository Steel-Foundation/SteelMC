//! Vanilla `LeashFenceKnotEntity`: the knot a lead ties around a fence.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_events::BLOCK_ATTACH;
use steel_registry::{sound_events, vanilla_entities, vanilla_items};
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Downcast as _, WorldAabb};

use crate::behavior::InteractionResult;
use crate::entity::block_attached::{self, BlockAttachment};
use crate::entity::component::definition::{Definition, InteractCtx};
use crate::entity::component::pipeline::{Flow, StageId};
use crate::entity::prelude::*;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityMoveError, EntityProps,
    RemovalReason, SharedEntity, next_entity_id,
};
use crate::player::Player;
use crate::world::World;

/// A leash knot tied to a fence. Never saved: leashed mobs store the fence
/// position and recreate the knot on load.
#[entity_behavior(class = "LeashFenceKnotEntity")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, no_save_data)]
#[entity(key = "steel:entity/leash_fence_knot", definition = Self::define)]
pub struct LeashFenceKnotEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    attachment: BlockAttachment,
}

impl LeashFenceKnotEntity {
    const INTERACT: StageId = StageId("leash_fence_knot.interact");

    fn define() -> Definition<Self> {
        let definition = block_attached::definition();
        Definition {
            interact: definition.interact.around(
                Self::INTERACT,
                |knot: &Self, ctx: &mut InteractCtx<'_>, inner| {
                    let (player, hand) = (ctx.player, ctx.hand);
                    let result = knot.interact_around(player, hand, || {
                        inner(knot, ctx);
                        ctx.result
                    });
                    ctx.result = result;
                    Flow::Continue
                },
            ),
            ..definition
        }
    }

    const ENTITY: EntityProps<Self> = EntityProps {
        notify_leashee_removed: |knot, _| {
            if knot.leashables_leashed_to().is_empty() {
                knot.set_removed(RemovalReason::Discarded);
            }
        },
        ..block_attached::entity_props()
    };

    /// Creates a knot from the generic entity factory path.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_attached(
            entity_type,
            id,
            BlockPos::containing(position.x, position.y, position.z),
            world,
        )
    }

    /// Creates a knot tied to the fence at `block_pos`.
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
            attachment: BlockAttachment::new(block_pos),
        }
    }

    /// Reconstructs a knot from saved base data, tied to the block it is in.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let position = load.position;
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            attachment: BlockAttachment::new(BlockPos::containing(
                position.x, position.y, position.z,
            )),
        }
    }

    /// Finds the knot tied to the fence at `pos`.
    #[must_use]
    pub fn get_knot(world: &World, pos: BlockPos) -> Option<SharedEntity> {
        let corner = pos.0.as_dvec3();
        let search_box = WorldAabb::new(
            corner.x - 1.0,
            corner.y - 1.0,
            corner.z - 1.0,
            corner.x + 1.0,
            corner.y + 1.0,
            corner.z + 1.0,
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

    /// Finds the knot tied to the fence at `pos`, or ties a new one.
    #[must_use]
    pub fn get_or_create_knot(world: &Arc<World>, pos: BlockPos) -> Option<SharedEntity> {
        if let Some(knot) = Self::get_knot(world, pos) {
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
        block_pos.0.as_dvec3() + DVec3::new(0.5, 0.375, 0.5)
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

    /// `LeashFenceKnotEntity.interact`: ties the player's leashed mobs to this
    /// fence, or hands the knot's mobs to the player.
    /// `LeashFenceKnotEntity.interact`: shears may cut the leashes first
    /// (`super`); then leashes move to or from the knot, or `super` runs.
    fn interact_around(
        &self,
        player: &Player,
        hand: InteractionHand,
        mut super_interact: impl FnMut() -> InteractionResult,
    ) -> InteractionResult {
        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };

        let holding_shears = player
            .inventory
            .lock()
            .get_item_in_hand(hand)
            .is(&vanilla_items::SHEARS);
        if holding_shears {
            let result = super_interact();
            if result == InteractionResult::Success {
                return result;
            }
        }

        let Some(knot) = world.get_entity_by_id(self.id()) else {
            return InteractionResult::Pass;
        };
        let mut attached_mob = false;
        for entity in player.leashables_leashed_to() {
            if let Some(leashable) = entity.as_leashable()
                && leashable.can_have_a_leash_attached_to(self)
            {
                leashable.set_leashed_to(&knot);
                attached_mob = true;
            }
        }

        let mut any_dropped = false;
        if !attached_mob && !player.is_secondary_use_active() {
            let Some(player_entity) = world.get_entity_by_id(player.id()) else {
                return InteractionResult::Pass;
            };
            for entity in self.leashables_leashed_to() {
                if let Some(leashable) = entity.as_leashable()
                    && leashable.can_have_a_leash_attached_to(player)
                {
                    leashable.set_leashed_to(&player_entity);
                    any_dropped = true;
                }
            }
        }

        if !attached_mob && !any_dropped {
            return super_interact();
        }

        self.game_event_with_source_entity(&BLOCK_ATTACH, Some(player));
        self.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
        InteractionResult::Success
    }
}

impl block_attached::Parts for LeashFenceKnotEntity {
    fn survives(&self) -> bool {
        self.level().is_some_and(|world| {
            world
                .get_block_state(self.block_pos())
                .get_block()
                .has_tag(&BlockTag::FENCES)
        })
    }

    fn drop_item(&self, _world: &World, _caused_by: Option<&dyn Entity>) {
        // The leashed mobs drop their leads from their own leash tick.
        self.play_sound(&sound_events::ITEM_LEAD_UNTIED, 1.0, 1.0);
    }

    fn recalculate_bounding_box(&self) -> Result<(), EntityMoveError> {
        let pos = self.block_pos();
        self.try_set_position_base(Self::knot_center(pos))?;
        self.base()
            .set_bounding_box(Self::knot_bounding_box(self.entity_type, pos));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use simdnbt::owned::NbtCompound;

    use super::*;

    #[test]
    fn knot_saves_no_block_pos() {
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
}
