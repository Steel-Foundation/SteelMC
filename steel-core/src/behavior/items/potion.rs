use std::borrow::Cow;
use std::sync::Arc;

use crate::behavior::item::finish_consuming_stack;
use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};
use crate::entity::LivingEntity;
use crate::entity::apply_potion_contents;
use crate::world::World;
use crate::world::game_event::GameEventContext;
use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::particle_type::ParticleData;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_game_events, vanilla_items, vanilla_particle_types,
    vanilla_potions,
};
use steel_utils::Direction;
use steel_utils::types::UpdateFlags;
use text_components::TextComponent;

use super::dynamic_name::potion_name;

/// Potion behavior providing Vanilla's potion-content-dependent name and
/// water default instance.
#[item_behavior]
pub struct PotionItem;

impl ItemBehavior for PotionItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }

    fn finish_using(
        &self,
        stack: &mut ItemStack,
        world: &Arc<World>,
        user: &dyn LivingEntity,
    ) -> ItemStack {
        let contents =
            stack.get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
        let duration_scale = stack.get_or_default(vanilla_components::POTION_DURATION_SCALE, 1.0);
        apply_potion_contents(&contents, world, user, duration_scale);
        finish_consuming_stack(stack, world, user)
    }

    fn get_default_instance(&self, item: ItemRef) -> ItemStack {
        potion_default_instance(item)
    }

    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        potion_use_on(context)
    }
}

pub(super) fn potion_default_instance(item: ItemRef) -> ItemStack {
    PotionContents::create_item_stack(item, &vanilla_potions::WATER)
}

/// Converts the block hit into mud when the held stack is a water bottle
/// over a `CONVERTABLE_TO_MUD` block.
pub(super) fn potion_use_on(context: &mut UseOnContext) -> InteractionResult {
    if context.hit_result.direction == Direction::Down {
        return InteractionResult::Pass;
    }

    let pos = context.hit_result.block_pos;
    let block_state = context.world.get_block_state(pos);
    if !block_state
        .get_block()
        .has_tag(&BlockTag::CONVERTABLE_TO_MUD)
    {
        return InteractionResult::Pass;
    }

    let is_water = context.inv.with_inventory(|inv| {
        inv.get_item_in_hand(context.hand)
            .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty())
            .is(&vanilla_potions::WATER)
    });
    if !is_water {
        return InteractionResult::Pass;
    }

    context.world.play_sound(
        &sound_events::ENTITY_GENERIC_SPLASH,
        SoundSource::Blocks,
        pos,
        1.0,
        1.0,
        None,
    );
    context.inv.with_inventory(|inv| {
        inv.apply_filled_result(
            context.hand,
            ItemStack::new(&vanilla_items::GLASS_BOTTLE),
            context.player.has_infinite_materials(),
            true,
        );
    });
    for _ in 0..5 {
        context.world.send_particles(
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
    context.world.play_sound(
        &sound_events::ITEM_BOTTLE_EMPTY,
        SoundSource::Blocks,
        pos,
        1.0,
        1.0,
        None,
    );
    context.world.game_event(
        &vanilla_game_events::FLUID_PLACE,
        pos,
        &GameEventContext::new(None, None),
    );
    context.world.set_block(
        pos,
        vanilla_blocks::MUD.default_state(),
        UpdateFlags::UPDATE_ALL,
    );

    InteractionResult::Success
}
