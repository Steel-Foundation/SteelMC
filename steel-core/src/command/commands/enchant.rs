//! Handler for the "enchant" command.
use std::sync::Arc;

use steel_registry::enchantment::EnchantmentRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt};
use steel_utils::Identifier;
use steel_utils::translations;
use text_components::translation::TranslatedMessage;
use text_components::{Modifier, TextComponent};

use std::borrow::Cow;

use crate::{
    command::{
        arguments::{
            enchantment::EnchantmentArgument, integer::IntegerArgument, player::PlayerArgument,
        },
        commands::{CommandHandlerBuilder, CommandHandlerDyn, argument},
        context::CommandContext,
        error::CommandError,
    },
    player::Player,
};

/// Handler for the "enchant" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["enchant"],
        "Enchants a player's selected item.",
        "minecraft:command.enchant",
    )
    .then(
        argument("targets", PlayerArgument::multiple()).then(
            argument("enchantment", EnchantmentArgument)
                .executes(
                    |(((), targets), enchantment): (((), Vec<Arc<Player>>), EnchantmentRef),
                     ctx: &mut CommandContext| {
                        enchant(&targets, enchantment, 1, ctx)
                    },
                )
                .then(
                    argument("level", IntegerArgument::bounded(Some(0), None)).executes(
                        |((((), targets), enchantment), level): (
                            (((), Vec<Arc<Player>>), EnchantmentRef),
                            i32,
                        ),
                         ctx: &mut CommandContext| {
                            enchant(&targets, enchantment, level, ctx)
                        },
                    ),
                ),
        ),
    )
}

/// Checks if the enchantment can be applied to this item (supported_items tag check).
fn can_enchant(enchantment: EnchantmentRef, item: &ItemStack) -> bool {
    let Some(tag_str) = enchantment.supported_items.strip_prefix('#') else {
        return false;
    };
    let tag_key = tag_str.strip_prefix("minecraft:").unwrap_or(tag_str);
    let tag = Identifier::vanilla(tag_key.to_owned());
    REGISTRY.items.is_in_tag(item.item, &tag)
}

/// Checks if the given enchantment is compatible with all existing enchantments on the item.
///
/// Vanilla checks `Enchantment.areCompatible` for each pair, which verifies:
/// - Not the same enchantment
/// - Neither enchantment's exclusive_set contains the other
fn is_enchantment_compatible(enchantment: EnchantmentRef, item: &ItemStack) -> bool {
    let Some(enchantments) = item.get_enchantments() else {
        return true;
    };

    for (existing_key, _) in enchantments.iter() {
        if *existing_key == enchantment.key {
            // Same enchantment is allowed (it will be upgraded)
            continue;
        }

        let Some(existing) = REGISTRY.enchantments.by_key(existing_key) else {
            continue;
        };

        if !are_compatible(enchantment, existing) {
            return false;
        }
    }

    true
}

/// Mirrors vanilla `Enchantment.areCompatible`: two enchantments are compatible if
/// neither's exclusive_set tag contains the other.
fn are_compatible(a: EnchantmentRef, b: EnchantmentRef) -> bool {
    if std::ptr::eq(a, b) {
        return false;
    }

    if let Some(set) = a.exclusive_set {
        if is_in_enchantment_tag(set, b) {
            return false;
        }
    }
    if let Some(set) = b.exclusive_set {
        if is_in_enchantment_tag(set, a) {
            return false;
        }
    }

    true
}

/// Checks if an enchantment is in a tag reference like `"#minecraft:exclusive_set/damage"`.
fn is_in_enchantment_tag(tag_ref: &str, enchantment: EnchantmentRef) -> bool {
    let Some(tag_str) = tag_ref.strip_prefix('#') else {
        return false;
    };
    let tag_key = tag_str.strip_prefix("minecraft:").unwrap_or(tag_str);
    let tag = Identifier::vanilla(tag_key.to_owned());
    REGISTRY.enchantments.is_in_tag(enchantment, &tag)
}

fn enchant(
    targets: &[Arc<Player>],
    enchantment: EnchantmentRef,
    level: i32,
    ctx: &mut CommandContext,
) -> Result<(), CommandError> {
    if level > enchantment.max_level as i32 {
        return Err(CommandError::CommandFailed(Box::new(
            translations::COMMANDS_ENCHANT_FAILED_LEVEL
                .message([
                    TextComponent::from(level.to_string()),
                    TextComponent::from(enchantment.max_level.to_string()),
                ])
                .into(),
        )));
    }

    let mut success = 0u32;
    let enchantment_key = enchantment.key.clone();

    for target in targets {
        let mut inv = target.inventory.lock();
        let item = inv.get_selected_item();

        if item.is_empty() {
            if targets.len() == 1 {
                return Err(CommandError::CommandFailed(Box::new(
                    translations::COMMANDS_ENCHANT_FAILED_ITEMLESS
                        .message([TextComponent::from(target.gameprofile.name.clone())])
                        .into(),
                )));
            }
            continue;
        }

        if !can_enchant(enchantment, item) || !is_enchantment_compatible(enchantment, item) {
            if targets.len() == 1 {
                let item_name = item.item.key.to_string();
                return Err(CommandError::CommandFailed(Box::new(
                    translations::COMMANDS_ENCHANT_FAILED_INCOMPATIBLE
                        .message([TextComponent::from(item_name)])
                        .into(),
                )));
            }
            continue;
        }

        let item = inv.get_selected_item_mut();
        item.set_enchantments(&[(enchantment_key.clone(), level.max(0) as u32)], false);
        success += 1;
    }

    if success == 0 {
        return Err(CommandError::CommandFailed(Box::new(
            translations::COMMANDS_ENCHANT_FAILED.msg().into(),
        )));
    }

    let enchantment_name = enchantment_display_name(enchantment, level);

    if targets.len() == 1 {
        ctx.sender.send_message(
            &translations::COMMANDS_ENCHANT_SUCCESS_SINGLE
                .message([
                    enchantment_name,
                    TextComponent::from(targets[0].gameprofile.name.clone()),
                ])
                .into(),
        );
    } else {
        ctx.sender.send_message(
            &translations::COMMANDS_ENCHANT_SUCCESS_MULTIPLE
                .message([
                    enchantment_name,
                    TextComponent::from(targets.len().to_string()),
                ])
                .into(),
        );
    }

    Ok(())
}

/// Builds a display name matching vanilla's `Enchantment.getFullname`:
/// translatable enchantment name + level suffix when level > 1 or max_level > 1.
fn enchantment_display_name(enchantment: EnchantmentRef, level: i32) -> TextComponent {
    let name_msg = TranslatedMessage {
        key: Cow::Owned(format!("enchantment.{}", enchantment.key)),
        args: None,
        fallback: None,
    };
    let mut component = TextComponent::translated(name_msg);

    if level != 1 || enchantment.max_level != 1 {
        let level_msg = TranslatedMessage {
            key: Cow::Owned(format!("enchantment.level.{level}")),
            args: None,
            fallback: None,
        };
        component = component
            .add_child(TextComponent::plain(" "))
            .add_child(TextComponent::translated(level_msg));
    }

    component
}
