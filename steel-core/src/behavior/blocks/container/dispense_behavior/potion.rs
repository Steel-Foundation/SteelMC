use glam::DVec3;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::data_components::PotionContents;
use steel_registry::data_components::vanilla_components::POTION_CONTENTS;
use steel_registry::fluid::FluidStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::ParticleData;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_game_events, vanilla_items, vanilla_particle_types,
    vanilla_potions,
};
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use super::DefaultDispenseBehavior;
use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::World;
use crate::world::game_event::GameEventContext;

const MUD_SPLASH_PARTICLE_COUNT: usize = 5;

pub struct GlassBottleDispenseBehavior;

impl DispenseItemBehavior for GlassBottleDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        let target_pos = pos.relative(state.get_value(FACING));
        // TODO: honey bottle from a beehive with honey_level >= 5 — needs
        // beehive honey-level tracking, which isn't ported yet.
        if world
            .get_block_state(target_pos)
            .get_fluid_state()
            .is_water()
        {
            world.game_event(
                &vanilla_game_events::FLUID_PICKUP,
                pos,
                &GameEventContext::new(None, None),
            );
            return PotionContents::create_item_stack(
                &vanilla_items::POTION,
                &vanilla_potions::WATER,
            );
        }

        DefaultDispenseBehavior.dispense(world, pos, state, item)
    }
}

/// Water bottles convert the block in front into mud.
///
/// Vanilla plays the splash particles, sound, and game event at the dispenser
/// position rather than the converted block; this is kept for parity.
pub struct PotionDispenseBehavior;

impl DispenseItemBehavior for PotionDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        if !item
            .get_or_default(POTION_CONTENTS, PotionContents::empty())
            .is(&vanilla_potions::WATER)
        {
            return DefaultDispenseBehavior.dispense(world, pos, state, item);
        }

        let target_pos = pos.relative(state.get_value(FACING));
        if !world
            .get_block_state(target_pos)
            .get_block()
            .has_tag(&BlockTag::CONVERTIBLE_TO_MUD)
        {
            return DefaultDispenseBehavior.dispense(world, pos, state, item);
        }

        for _ in 0..MUD_SPLASH_PARTICLE_COUNT {
            world.send_particles(
                ParticleData::simple(&vanilla_particle_types::SPLASH),
                DVec3::new(
                    f64::from(pos.x()) + rand::random::<f64>(),
                    f64::from(pos.y()) + 1.0,
                    f64::from(pos.z()) + rand::random::<f64>(),
                ),
                1,
                DVec3::ZERO,
                1.0,
            );
        }
        world.play_block_sound(&sound_events::ITEM_BOTTLE_EMPTY, pos, 1.0, 1.0, None);
        world.game_event(
            &vanilla_game_events::FLUID_PLACE,
            pos,
            &GameEventContext::new(None, None),
        );
        world.set_block(
            target_pos,
            vanilla_blocks::MUD.default_state(),
            UpdateFlags::UPDATE_ALL,
        );

        ItemStack::new(&vanilla_items::GLASS_BOTTLE)
    }
}
