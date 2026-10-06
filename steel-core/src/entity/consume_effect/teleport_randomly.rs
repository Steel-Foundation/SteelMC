//! `TeleportRandomlyConsumeEffect` behavior (chorus fruit).

use std::sync::Arc;

use steel_protocol::packets::game::SoundSource;
use steel_registry::consume_effect::{ConsumeEffectData, TeleportRandomlyConsumeEffect};
use steel_registry::{sound_events, vanilla_game_events};

use super::ConsumeEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Mirrors vanilla `TeleportRandomlyConsumeEffect.apply`.
pub struct TeleportRandomlyBehavior;

impl ConsumeEffectBehavior for TeleportRandomlyBehavior {
    fn apply(&self, effect: &ConsumeEffectData, world: &Arc<World>, user: &dyn LivingEntity) {
        let Some(teleport) = effect.downcast_ref::<TeleportRandomlyConsumeEffect>() else {
            return;
        };
        teleport_randomly(*teleport, world, user);
    }
}

/// Tries up to 16 random nearby positions, delegating each attempt to
/// `LivingEntity::random_teleport` (vanilla `Entity.randomTeleport`), and
/// stops at the first one that lands.
///
// TODO(26.3): Vanilla snapshot 26.3 adds the block tag
// `#consumable_does_not_teleport_to` — "blocks that entities do not
// teleport to when they consume food that teleports randomly when eaten"
// (empty by default). Once that tag exists in the registry extraction, a
// landing block tagged with it must be rejected here, the same way an
// unloaded/non-solid candidate already is — this is specific to this
// consume-effect path, not the shared `Entity.randomTeleport` primitive
// (Enderman's own random teleport is unaffected by this tag).
fn teleport_randomly(
    effect: TeleportRandomlyConsumeEffect,
    world: &Arc<World>,
    user: &dyn LivingEntity,
) {
    let diameter = f64::from(effect.diameter());
    let min_y = f64::from(world.get_min_y());
    let max_y = f64::from(world.get_min_y() + world.dimension_type.logical_height - 1);

    for _ in 0..16 {
        let origin = user.position();
        let x = origin.x + (rand::random::<f64>() - 0.5) * diameter;
        let y = (origin.y + (rand::random::<f64>() - 0.5) * diameter).clamp(min_y, max_y);
        let z = origin.z + (rand::random::<f64>() - 0.5) * diameter;

        if user.is_passenger() {
            user.stop_riding();
        }

        let old_pos = user.position();
        if !user.random_teleport(world, x, y, z, true) {
            continue;
        }

        world.game_event_at(
            &vanilla_game_events::TELEPORT,
            old_pos,
            &GameEventContext::new(Some(user.as_entity_event_source()), None),
        );
        // TODO: Play `FOX_TELEPORT` on `SoundSource::Neutral` instead once Fox
        // is implemented, mirroring vanilla `TeleportRandomlyConsumeEffect.apply`.
        world.play_sound_at(
            &sound_events::ITEM_CHORUS_FRUIT_TELEPORT,
            SoundSource::Players,
            user.position(),
            1.0,
            1.0,
            None,
        );
        user.reset_fall_distance();
        user.reset_current_impulse_context();
        return;
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::consume_effect::TeleportRandomlyConsumeEffect;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::teleport_randomly;
    use crate::behavior::init_behaviors;
    use crate::entity::Entity;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    /// With no solid ground anywhere in range, every landing attempt must
    /// fail and the player must stay exactly where they started — mirroring
    /// vanilla `Entity.randomTeleport` reverting to the original position
    /// when no candidate lands.
    #[test]
    fn teleport_randomly_leaves_the_player_in_place_with_no_valid_landing() {
        init_vanilla_registry();
        let world = fresh_test_world("teleport_randomly_no_valid_landing");
        for x in -1..=0 {
            for z in -1..=0 {
                insert_ready_full_chunk(&world, ChunkPos::new(x, z));
            }
        }
        let player = TestPlayerBuilder::new(world.clone(), "Test", 1).build();
        let origin = player.position();

        teleport_randomly(
            TeleportRandomlyConsumeEffect::default_value(),
            &world,
            player.as_ref(),
        );

        assert_eq!(player.position(), origin);
    }

    /// With solid ground everywhere in range, the player must land on top of
    /// it, within the effect's diameter of the origin. Mirrors vanilla
    /// `TeleportRandomlyConsumeEffect.apply` picking the first safe landing.
    #[test]
    fn teleport_randomly_lands_on_solid_ground_within_diameter() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("teleport_randomly_valid_landing");
        for x in -1..=0 {
            for z in -1..=0 {
                insert_ready_full_chunk(&world, ChunkPos::new(x, z));
            }
        }
        for x in -8..8 {
            for z in -8..8 {
                world.set_block(
                    BlockPos::new(x, -1, z),
                    vanilla_blocks::STONE.default_state(),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        }
        let player = TestPlayerBuilder::new(world.clone(), "Test", 1).build();
        let origin = player.position();

        teleport_randomly(
            TeleportRandomlyConsumeEffect::default_value(),
            &world,
            player.as_ref(),
        );

        let landed = player.position();
        assert_ne!(landed, origin);
        assert!((landed.x - origin.x).abs() <= 8.0);
        assert!((landed.z - origin.z).abs() <= 8.0);
        // The landing loop preserves the fractional part of the candidate Y
        // (see `find_ground_y`), so a floor at block Y = -1 always lands the
        // player somewhere in [0, 1) above it.
        assert!((0.0..1.0).contains(&landed.y));
    }
}
