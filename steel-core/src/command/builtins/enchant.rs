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
use crate::entity::{Living, SharedEntity};

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
    let level = context.integer("level")?;
    enchant(context, level)
}

fn enchant(
    context: &SteelCommandContext<CommandSource>,
    level: i32,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let enchantment = context.enchantment("enchantment")?;
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

        match enchant_main_hand(&living, enchantment, level) {
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
    target: &Living<'_>,
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

#[cfg(test)]
mod tests {
    use crate::entity::EntitySyncedData;
    use crate::entity::LivingParts as _;
    use crate::entity::LivingProps;
    use crate::entity::component::Has;
    use crate::test_support::synced_with_health;
    use std::sync::Weak;
    use steel_registry::vanilla_entity_data::LivingEntityData;
    use steel_utils::DowncastType as _;
    use steel_utils::DowncastTypeKey;
    use steel_utils::ErasedType;

    use glam::DVec3;
    use steel_registry::{
        entity_type::EntityTypeRef, equipment::EquipmentSlot, init_vanilla_registry,
        item_stack::ItemStack, vanilla_enchantments, vanilla_entities, vanilla_items,
    };
    use steel_utils::locks::CheckedMutex;

    use super::super::create_dispatcher;
    use super::{EnchantTargetResult, enchant_main_hand};
    use crate::entity::{EntityHooks, EntityProps, living_entity};
    use crate::{
        command::{
            brigadier::{ArgumentType, CommandDispatcher, NodeId},
            execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
        },
        entity::{Entity, EntityBase, LivingEntityBase},
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
        init_vanilla_registry();
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
        init_vanilla_registry();
        let target = TestLivingEntity::new(&vanilla_entities::ZOMBIE);
        target.equip(ItemStack::new(&vanilla_items::DIAMOND_SWORD));

        assert_eq!(
            enchant_main_hand(&target.living(), &vanilla_enchantments::SHARPNESS, 1),
            EnchantTargetResult::Enchanted
        );
        assert_eq!(target.main_hand_enchantment_level(), 1);
        assert!(matches!(
            enchant_main_hand(&target.living(), &vanilla_enchantments::SHARPNESS, 2),
            EnchantTargetResult::Incompatible(_)
        ));
        assert_eq!(target.main_hand_enchantment_level(), 1);
    }

    struct TestLivingEntity {
        base: EntityBase,
        living_base: LivingEntityBase,
        entity_data: CheckedMutex<LivingEntityData>,
        entity_type: EntityTypeRef,
    }

    impl TestLivingEntity {
        fn new(entity_type: EntityTypeRef) -> Self {
            Self {
                base: EntityBase::new(1, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                living_base: LivingEntityBase::without_synced_data(
                    entity_type,
                    &TestLivingEntity::LIVING,
                ),
                entity_data: CheckedMutex::new(synced_with_health(LivingEntityData::new(), 20.0)),
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
            self.living()
                .with_equipment_slot(EquipmentSlot::MainHand, &mut |item| {
                    level = item.get_enchantment_level(&vanilla_enchantments::SHARPNESS.key);
                });
            level
        }
    }

    crate::entity::impl_test_downcast_type!(TestLivingEntity);

    impl TestLivingEntity {
        const ENTITY: EntityProps<Self> = EntityProps {
            ..living_entity::entity_props()
        };
    }

    impl Entity for TestLivingEntity {
        fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
            Some(&self.entity_data)
        }

        fn erased_component(&self, key: DowncastTypeKey) -> Option<&dyn ErasedType> {
            (key == LivingEntityBase::TYPE_KEY).then_some(&self.living_base as &dyn ErasedType)
        }

        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn entity_hooks(&self) -> &'static dyn EntityHooks {
            &Self::ENTITY
        }
    }

    impl TestLivingEntity {
        const LIVING: LivingProps<Self> = LivingProps {
            absorption_amount: Self::living_get_absorption_amount,
            set_absorption_amount: Self::living_set_absorption_amount,
            ..LivingProps::new()
        };

        #[expect(
            clippy::unused_self,
            reason = "kept as a method of the view for callers; the body does not need the entity"
        )]
        fn living_get_absorption_amount(&self) -> f32 {
            0.0
        }

        #[expect(
            clippy::unused_self,
            reason = "kept as a method of the view for callers; the body does not need the entity"
        )]
        fn living_set_absorption_amount(&self, _amount: f32) {}
    }

    impl Has<LivingEntityBase> for TestLivingEntity {
        fn component(&self) -> &LivingEntityBase {
            &self.living_base
        }
    }
}
