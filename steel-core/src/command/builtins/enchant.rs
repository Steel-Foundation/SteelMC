//! Vanilla main-hand enchantment command.

use std::borrow::Cow;

use steel_registry::{
    REGISTRY, TaggedRegistryExt as _,
    enchantment::{Enchantment, EnchantmentRef},
    equipment::EquipmentSlot,
    vanilla_enchantment_tags::EnchantmentTag,
};
use steel_utils::{Identifier, translations};
use text_components::{Modifier, TextComponent, format::Color, translation::TranslatedMessage};

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::{LivingEntity, SharedEntity};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("enchant"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("enchant").then(
        argument("targets", SteelArgumentType::entities()).then(
            argument("enchantment", SteelArgumentType::enchantment())
                .executes(enchant_default_level)
                .then(
                    argument("level", ArgumentType::integer(0, i32::MAX))
                        .executes(enchant_with_level),
                ),
        ),
    )
}

fn enchant_default_level(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    enchant(context, 1)
}

fn enchant_with_level(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let Some(level) = context.integer("level") else {
        return Err(missing_argument("level"));
    };
    enchant(context, level)
}

fn enchant(
    context: &SteelCommandContext<CommandSource>,
    level: i32,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let Some(enchantment) = context.enchantment("enchantment") else {
        return Err(missing_argument("enchantment"));
    };
    let level = u32::try_from(level)
        .map_err(|_| CommandSyntaxError::dynamic("Enchantment level cannot be negative"))?;
    if level > enchantment.max_level {
        let message = translations::COMMANDS_ENCHANT_FAILED_LEVEL
            .message([level.to_string(), enchantment.max_level.to_string()])
            .component();
        return Err(CommandSyntaxError::dynamic(message));
    }

    let mut success = 0usize;
    for target in &targets {
        let Some(living) = target.as_living_entity() else {
            if targets.len() == 1 {
                return Err(not_living_error(target));
            }
            continue;
        };

        match enchant_main_hand(living, enchantment, level) {
            EnchantTargetResult::Enchanted => success += 1,
            EnchantTargetResult::Itemless if targets.len() == 1 => {
                return Err(itemless_error(target));
            }
            EnchantTargetResult::Incompatible(item_name) if targets.len() == 1 => {
                return Err(incompatible_error(item_name));
            }
            EnchantTargetResult::Itemless | EnchantTargetResult::Incompatible(_) => {}
        }
    }

    if success == 0 {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_ENCHANT_FAILED,
        )));
    }

    let enchantment_name = enchantment_display_name(enchantment, level);
    let message = if let [target] = targets.as_slice() {
        translations::COMMANDS_ENCHANT_SUCCESS_SINGLE
            .message([
                enchantment_name,
                TextComponent::plain(target.plain_text_name()),
            ])
            .component()
    } else {
        translations::COMMANDS_ENCHANT_SUCCESS_MULTIPLE
            .message([
                enchantment_name,
                TextComponent::from(targets.len().to_string()),
            ])
            .component()
    };
    context.source().send_success(&message, true);

    i32::try_from(success).map_err(|_| {
        CommandSyntaxError::dynamic("Enchanted entity count exceeds the command result range")
    })
}

#[derive(Debug, PartialEq, Eq)]
enum EnchantTargetResult {
    Enchanted,
    Itemless,
    Incompatible(Box<str>),
}

fn enchant_main_hand(
    target: &dyn LivingEntity,
    enchantment: EnchantmentRef,
    level: u32,
) -> EnchantTargetResult {
    let mut result = EnchantTargetResult::Itemless;
    target.with_equipment_slot(EquipmentSlot::MainHand, &mut |item| {
        if item.is_empty() {
            return;
        }
        if !enchantment.can_enchant(item.item())
            || !Enchantment::is_compatible_with_existing(enchantment, item)
        {
            result =
                EnchantTargetResult::Incompatible(item.item().key.to_string().into_boxed_str());
            return;
        }
        result = EnchantTargetResult::Enchanted;
    });

    if result == EnchantTargetResult::Enchanted {
        target.with_equipment_slot_mut(EquipmentSlot::MainHand, &mut |item| {
            item.upgrade_enchantment(enchantment.key.clone(), level);
        });
    }
    result
}

fn not_living_error(target: &SharedEntity) -> CommandSyntaxError {
    let message = translations::COMMANDS_ENCHANT_FAILED_ENTITY
        .message([TextComponent::plain(target.plain_text_name())])
        .component();
    CommandSyntaxError::dynamic(message)
}

fn itemless_error(target: &SharedEntity) -> CommandSyntaxError {
    let message = translations::COMMANDS_ENCHANT_FAILED_ITEMLESS
        .message([TextComponent::plain(target.plain_text_name())])
        .component();
    CommandSyntaxError::dynamic(message)
}

fn incompatible_error(item_name: Box<str>) -> CommandSyntaxError {
    let message = translations::COMMANDS_ENCHANT_FAILED_INCOMPATIBLE
        .message([TextComponent::plain(String::from(item_name))])
        .component();
    CommandSyntaxError::dynamic(message)
}

fn enchantment_display_name(enchantment: EnchantmentRef, level: u32) -> TextComponent {
    let color = if REGISTRY
        .enchantments
        .is_in_tag(enchantment, &EnchantmentTag::CURSE)
    {
        Color::Red
    } else {
        Color::Gray
    };
    let mut component = TextComponent::translated(TranslatedMessage {
        key: Cow::Owned(format!(
            "enchantment.{}.{}",
            enchantment.key.namespace, enchantment.key.path
        )),
        args: None,
        fallback: None,
    })
    .color(color);

    if level != 1 || enchantment.max_level != 1 {
        component =
            component
                .add_child(TextComponent::plain(" "))
                .add_child(TextComponent::translated(TranslatedMessage {
                    key: Cow::Owned(format!("enchantment.level.{level}")),
                    args: None,
                    fallback: None,
                }));
    }
    component
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{
        entity_type::EntityTypeRef, equipment::EquipmentSlot, item_stack::ItemStack,
        test_support::init_test_registry, vanilla_enchantments, vanilla_entities, vanilla_items,
    };
    use steel_utils::locks::SyncMutex;

    use super::super::create_dispatcher;
    use super::{EnchantTargetResult, enchant_main_hand};
    use crate::{
        command::{
            brigadier::{ArgumentType, CommandDispatcher, NodeId},
            execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
        },
        entity::{Entity, EntityBase, LivingEntity, LivingEntityBase},
    };

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child {name} should exist");
        };
        child
    }

    #[test]
    fn enchant_graph_uses_all_entities_and_an_enchantment_resource() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let enchant = child(&dispatcher, dispatcher.root(), "enchant");
        let targets = child(&dispatcher, enchant, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::entities())
        );

        let enchantment = child(&dispatcher, targets, "enchantment");
        assert_eq!(
            dispatcher
                .node(enchantment)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::enchantment())
        );
        assert!(matches!(
            dispatcher.node(enchantment),
            Some(node) if node.is_executable()
        ));

        let level = child(&dispatcher, enchantment, "level");
        assert_eq!(
            dispatcher.node(level).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(0, i32::MAX)))
        );
    }

    #[test]
    fn enchant_main_hand_applies_once_and_then_rejects_the_same_enchantment() {
        init_test_registry();
        let target = TestLivingEntity::new(&vanilla_entities::ZOMBIE);
        target.equip(ItemStack::new(&vanilla_items::ITEMS.diamond_sword));

        assert_eq!(
            enchant_main_hand(&target, &vanilla_enchantments::SHARPNESS, 1),
            EnchantTargetResult::Enchanted
        );
        assert_eq!(target.main_hand_enchantment_level(), 1);
        assert!(matches!(
            enchant_main_hand(&target, &vanilla_enchantments::SHARPNESS, 2),
            EnchantTargetResult::Incompatible(_)
        ));
        assert_eq!(target.main_hand_enchantment_level(), 1);
    }

    struct TestLivingEntity {
        base: EntityBase,
        living_base: LivingEntityBase,
        health: SyncMutex<f32>,
        entity_type: EntityTypeRef,
    }

    impl TestLivingEntity {
        fn new(entity_type: EntityTypeRef) -> Self {
            Self {
                base: EntityBase::new(1, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                living_base: LivingEntityBase::new(entity_type),
                health: SyncMutex::new(20.0),
                entity_type,
            }
        }

        fn equip(&self, stack: ItemStack) {
            self.living_base
                .equipment()
                .lock()
                .set(EquipmentSlot::MainHand, stack);
        }

        fn main_hand_enchantment_level(&self) -> i32 {
            let mut level = 0;
            self.with_equipment_slot(EquipmentSlot::MainHand, &mut |item| {
                level = item.get_enchantment_level(&vanilla_enchantments::SHARPNESS.key);
            });
            level
        }
    }

    crate::entity::impl_test_downcast_type!(TestLivingEntity);

    impl Entity for TestLivingEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
            Some(self)
        }
    }

    impl LivingEntity for TestLivingEntity {
        fn living_base(&self) -> &LivingEntityBase {
            &self.living_base
        }

        fn get_health(&self) -> f32 {
            *self.health.lock()
        }

        fn set_health(&self, health: f32) {
            *self.health.lock() = health;
        }

        fn get_absorption_amount(&self) -> f32 {
            0.0
        }

        fn set_absorption_amount(&self, _amount: f32) {}
    }
}
