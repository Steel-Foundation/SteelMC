use std::borrow::Cow;

use steel_registry::{
    data_components::vanilla_components::{ITEM_NAME, POTION_CONTENTS},
    item_stack::ItemStack,
};
use text_components::{TextComponent, content::Content, translation::TranslatedMessage};

pub(super) fn default_name(stack: &ItemStack) -> Cow<'_, TextComponent> {
    stack
        .get(ITEM_NAME)
        .map_or_else(|| Cow::Owned(TextComponent::new()), Cow::Borrowed)
}

pub(super) fn description_id(stack: &ItemStack) -> Option<&str> {
    let Content::Translate(message) = &stack.item().components.get_ref(ITEM_NAME)?.content else {
        return None;
    };
    Some(message.key.as_ref())
}

pub(super) fn translated(
    key: String,
    args: Option<Box<[TextComponent]>>,
) -> Cow<'static, TextComponent> {
    Cow::Owned(
        TranslatedMessage {
            key: Cow::Owned(key),
            fallback: None,
            args,
        }
        .component(),
    )
}

pub(super) fn potion_name(stack: &ItemStack) -> Cow<'_, TextComponent> {
    let Some(contents) = stack.get(POTION_CONTENTS) else {
        return default_name(stack);
    };
    let Some(description_id) = description_id(stack) else {
        return default_name(stack);
    };
    let suffix = contents
        .custom_name()
        .or_else(|| contents.potion().map(|potion| potion.value().name))
        .unwrap_or("empty");
    translated(format!("{description_id}.effect.{suffix}"), None)
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        REGISTRY, RegistryExt, RegistryReference,
        data_components::{
            components::{GlobalPos, LodestoneTracker, PotionContents},
            vanilla_components::{
                BASE_COLOR, ITEM_NAME, LODESTONE_TRACKER, POTION_CONTENTS, PROFILE,
            },
        },
        dye_color::DyeColor,
        init_vanilla_registry,
        item_stack::ItemStack,
        resolvable_profile::{PlayerSkinPatch, ResolvableProfile},
        vanilla_items,
    };
    use steel_utils::{BlockPos, Identifier};
    use text_components::{TextComponent, content::Content};

    use crate::behavior::{ITEM_BEHAVIORS, init_behaviors};

    fn translated_key(stack: &ItemStack) -> String {
        let name = ITEM_BEHAVIORS.hover_name(stack);
        let Content::Translate(message) = &name.content else {
            panic!("dynamic item name should be translated");
        };
        message.key.to_string()
    }

    #[test]
    fn every_mc26_dynamic_item_name_override_uses_stack_components() {
        init_vanilla_registry();
        init_behaviors();
        for potion_key in ["long_swiftness", "strong_swiftness"] {
            let potion = REGISTRY
                .potions
                .by_key(&Identifier::vanilla_static(potion_key))
                .expect("swiftness potion variant should be registered");
            for (item, expected) in [
                (
                    &*vanilla_items::POTION,
                    "item.minecraft.potion.effect.swiftness",
                ),
                (
                    &*vanilla_items::SPLASH_POTION,
                    "item.minecraft.splash_potion.effect.swiftness",
                ),
                (
                    &*vanilla_items::LINGERING_POTION,
                    "item.minecraft.lingering_potion.effect.swiftness",
                ),
                (
                    &*vanilla_items::TIPPED_ARROW,
                    "item.minecraft.tipped_arrow.effect.swiftness",
                ),
            ] {
                let mut stack = ItemStack::new(item);
                stack.set(
                    POTION_CONTENTS,
                    PotionContents::new(
                        Some(RegistryReference::new(potion)),
                        None,
                        Vec::new(),
                        None,
                    ),
                );
                assert_eq!(translated_key(&stack), expected);
            }
        }

        let mut compass = ItemStack::new(&vanilla_items::COMPASS);
        compass.set(
            LODESTONE_TRACKER,
            LodestoneTracker::new(
                Some(GlobalPos::new(
                    Identifier::vanilla_static("overworld"),
                    BlockPos::new(1, 2, 3),
                )),
                true,
            ),
        );
        assert_eq!(translated_key(&compass), "item.minecraft.lodestone_compass");

        let mut shield = ItemStack::new(&vanilla_items::SHIELD);
        shield.set(BASE_COLOR, DyeColor::Blue);
        assert_eq!(translated_key(&shield), "item.minecraft.shield.blue");

        let mut head = ItemStack::new(&vanilla_items::PLAYER_HEAD);
        head.set(
            PROFILE,
            ResolvableProfile::dynamic_name("Notch".to_owned(), PlayerSkinPatch::default())
                .expect("valid profile name"),
        );
        assert_eq!(translated_key(&head), "block.minecraft.player_head.named");
        let name = ITEM_BEHAVIORS.hover_name(&head);
        let Content::Translate(message) = &name.content else {
            panic!("named player head should be translated");
        };
        assert_eq!(
            message
                .args
                .as_deref()
                .and_then(|args| args.first())
                .map(ToString::to_string)
                .as_deref(),
            Some("Notch")
        );
    }

    #[test]
    fn air_name_ignores_stack_item_name_patch() {
        init_vanilla_registry();
        init_behaviors();
        let mut air = ItemStack::new(&vanilla_items::AIR);
        air.set(ITEM_NAME, TextComponent::plain("patched"));
        let Some(prototype_name) = vanilla_items::AIR.components.get_ref(ITEM_NAME) else {
            panic!("air item prototype should have a name");
        };

        assert_eq!(ITEM_BEHAVIORS.hover_name(&air).as_ref(), prototype_name);
    }

    #[test]
    fn dynamic_name_prefix_ignores_stack_item_name_patch() {
        init_vanilla_registry();
        init_behaviors();
        let healing = REGISTRY
            .potions
            .by_key(&Identifier::vanilla_static("healing"))
            .expect("healing potion should be registered");
        let mut potion = ItemStack::new(&vanilla_items::POTION);
        potion.set(
            POTION_CONTENTS,
            PotionContents::new(
                Some(RegistryReference::new(healing)),
                None,
                Vec::new(),
                None,
            ),
        );
        potion.set(ITEM_NAME, TextComponent::plain("patched"));

        assert_eq!(
            translated_key(&potion),
            "item.minecraft.potion.effect.healing"
        );
    }
}
