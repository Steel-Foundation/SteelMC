//! Vanilla `DyeItem` behavior: dyes an alive, unsheared sheep and sign text.

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::vanilla_components::DYE;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events::ITEM_DYE_USE;
use steel_utils::Downcast as _;
use steel_utils::types::{InteractionHand, SignTextSlot};

use crate::behavior::{InteractionResult, ItemBehavior, SignApplicator};
use crate::block_entity::{BlockEntity as _, entities::SignBlockEntity};
use crate::entity::entities::SheepEntity;
use crate::entity::{Entity, LivingEntity};
use crate::player::Player;
use crate::world::World;

/// Behavior for the sixteen dye items (`DyeItem`).
///
/// Ports vanilla `DyeItem.interactLivingEntity` and `DyeItem.tryApplyToSign`:
/// - Dying a sheep plays `DYE_USE`, sets the sheep's wool color, and consumes one dye.
/// - Dyeing a sign updates the facing text color and plays `DYE_USE` at `SoundSource::Blocks`.
#[item_behavior(class = "DyeItem")]
pub struct DyeItem;

impl ItemBehavior for DyeItem {
    fn as_sign_applicator(&self) -> Option<&dyn SignApplicator> {
        Some(self)
    }

    fn interact_living_entity(
        &self,
        stack: &mut ItemStack,
        _player: &Player,
        target: &dyn LivingEntity,
        _hand: InteractionHand,
    ) -> InteractionResult {
        let Some(dye_color) = stack.get(DYE).copied() else {
            return InteractionResult::Pass;
        };
        let Some(sheep) = target.downcast_ref::<SheepEntity>() else {
            return InteractionResult::Pass;
        };

        if !Entity::is_alive(sheep) || sheep.is_sheared() || sheep.color() == dye_color {
            return InteractionResult::Pass;
        }

        sheep.play_sound(&ITEM_DYE_USE, 1.0, 1.0);
        sheep.set_color(dye_color);
        stack.shrink_one();

        InteractionResult::Success
    }
}

impl SignApplicator for DyeItem {
    fn try_apply_to_sign(
        &self,
        world: &World,
        sign: &SignBlockEntity,
        slot: SignTextSlot,
        item: &ItemStack,
        _player: &Player,
    ) -> bool {
        let Some(dye_color) = item.get(DYE).copied() else {
            return false;
        };

        if !sign.set_color(slot, dye_color) {
            return false;
        }

        world.play_sound(
            &ITEM_DYE_USE,
            SoundSource::Blocks,
            sign.get_block_pos(),
            1.0,
            1.0,
            None,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::{DyeColor, init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::BlockPos;
    use text_components::TextComponent;

    use super::*;
    use crate::block_entity::entities::SignText;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};

    #[test]
    fn dye_sign_applicator_updates_color_and_respects_existing_color() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("dye_sign_applicator");
        let world = &world_fixture.world;
        let sign = SignBlockEntity::new(
            Arc::downgrade(world),
            BlockPos::new(0, 64, 0),
            vanilla_blocks::OAK_SIGN.default_state(),
        );

        let mut dye_stack = ItemStack::new(&vanilla_items::RED_DYE);
        dye_stack.set(DYE, DyeColor::Red);

        let player = TestPlayerBuilder::new(Arc::clone(world), "SignDyer", 1).build();
        let applicator = DyeItem;

        assert!(applicator.try_apply_to_sign(
            world,
            &sign,
            SignTextSlot::Front,
            &dye_stack,
            &player
        ));
        assert_eq!(sign.get_text(SignTextSlot::Front).color, DyeColor::Red);
        assert!(!applicator.try_apply_to_sign(
            world,
            &sign,
            SignTextSlot::Front,
            &dye_stack,
            &player
        ));

        let mut empty_text = SignText::new();
        assert!(!applicator.can_apply_to_sign(&empty_text, &dye_stack, &player));

        empty_text.set_message(0, TextComponent::plain("Hello"));
        assert!(applicator.can_apply_to_sign(&empty_text, &dye_stack, &player));
    }
}
