//! Shared item and data-component predicate evaluation.
use std::cmp::Ordering;
use steel_registry::{
    REGISTRY, RegistryExt as _,
    attribute::{AttributeModifierOperation, AttributeRef},
    data_component_predicate as registered,
    data_components::{
        Component, ComponentData, DataComponentType, PotionContents, vanilla_components,
    },
    enchantment::EnchantmentRef,
    equipment::EquipmentSlotGroup,
    item_predicate::{DoubleBounds, IntBounds, ItemPredicate as RegisteredItemPredicate},
    item_stack::ItemStack,
    items::ItemRef,
};
use steel_utils::{DowncastType, Identifier};
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DataComponentPredicate {
    CustomData(vanilla_components::CustomData),
    Damage(DamagePredicate),
    Enchantments {
        stored: bool,
        predicates: Vec<EnchantmentPredicate>,
    },
    AttributeModifiers(AttributeModifiersPredicate),
    Potions(registered::PotionsPredicate),
    Container(registered::ContainerPredicate),
    Bundle(registered::BundlePredicate),
    FireworkExplosion(registered::FireworkExplosionPredicate),
    Fireworks(registered::FireworksPredicate),
    WritableBook(registered::WritableBookPredicate),
    WrittenBook(registered::WrittenBookPredicate),
    Trim(registered::TrimPredicate),
    JukeboxPlayable(registered::JukeboxPlayablePredicate),
    VillagerType(registered::VillagerTypePredicate),
}

pub(crate) trait ItemInstanceView {
    fn item_ref(&self) -> ItemRef;
    fn item_count(&self) -> i32;
    fn effective_value_raw(&self, key: &Identifier) -> Option<&ComponentData>;

    fn component<T: Component + DowncastType>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<&T> {
        self.effective_value_raw(component.key())
            .and_then(ComponentData::downcast_ref::<T>)
    }
}

impl ItemInstanceView for ItemStack {
    fn item_ref(&self) -> ItemRef {
        self.item()
    }

    fn item_count(&self) -> i32 {
        self.count()
    }

    fn effective_value_raw(&self, key: &Identifier) -> Option<&ComponentData> {
        self.get_effective_value_raw(key)
    }
}

impl ItemInstanceView for steel_registry::ItemStackTemplate {
    fn item_ref(&self) -> ItemRef {
        self.item()
    }

    fn item_count(&self) -> i32 {
        self.count()
    }

    fn effective_value_raw(&self, key: &Identifier) -> Option<&ComponentData> {
        self.get_effective_value_raw(key)
    }
}

impl DataComponentPredicate {
    #[expect(
        clippy::too_many_lines,
        reason = "keeping every registered predicate variant in one match makes coverage auditable"
    )]
    pub(crate) fn matches<S: ItemInstanceView + ?Sized>(&self, stack: &S) -> bool {
        match self {
            Self::CustomData(expected) => stack
                .component(vanilla_components::CUSTOM_DATA)
                .cloned()
                .unwrap_or_default()
                .matched_by(expected.as_compound()),
            Self::Damage(predicate) => predicate.matches(stack),
            Self::Enchantments { stored, predicates } => {
                let enchantments = if *stored {
                    stack.component(vanilla_components::STORED_ENCHANTMENTS)
                } else {
                    stack.component(vanilla_components::ENCHANTMENTS)
                };
                enchantments.is_some_and(|enchantments| {
                    predicates
                        .iter()
                        .all(|predicate| predicate.matches(enchantments))
                })
            }
            Self::AttributeModifiers(predicate) => stack
                .component(vanilla_components::ATTRIBUTE_MODIFIERS)
                .is_some_and(|modifiers| predicate.matches(modifiers)),
            Self::Potions(predicate) => stack
                .component(vanilla_components::POTION_CONTENTS)
                .and_then(PotionContents::potion)
                .is_some_and(|potion| predicate.potions().contains(potion.value())),
            Self::Container(predicate) => stack
                .component(vanilla_components::CONTAINER)
                .is_some_and(|contents| {
                    predicate.items().is_none_or(|items| {
                        collection_matches(
                            items,
                            contents.items().iter().filter_map(Option::as_ref),
                            registered_item_predicate_matches,
                        )
                    })
                }),
            Self::Bundle(predicate) => stack
                .component(vanilla_components::BUNDLE_CONTENTS)
                .is_some_and(|contents| {
                    predicate.items().is_none_or(|items| {
                        collection_matches(
                            items,
                            contents.items().iter(),
                            registered_item_predicate_matches,
                        )
                    })
                }),
            Self::FireworkExplosion(predicate) => stack
                .component(vanilla_components::FIREWORK_EXPLOSION)
                .is_some_and(|explosion| firework_matches(predicate.predicate(), explosion)),
            Self::Fireworks(predicate) => stack
                .component(vanilla_components::FIREWORKS)
                .is_some_and(|fireworks| {
                    int_bounds_matches(predicate.flight_duration(), fireworks.flight_duration())
                        && predicate.explosions().is_none_or(|explosions| {
                            collection_matches(
                                explosions,
                                fireworks.explosions().iter(),
                                firework_matches,
                            )
                        })
                }),
            Self::WritableBook(predicate) => stack
                .component(vanilla_components::WRITABLE_BOOK_CONTENT)
                .is_some_and(|book| {
                    predicate.pages().is_none_or(|pages| {
                        collection_matches(pages, book.pages().iter(), |predicate, page| {
                            predicate.contents() == page.raw()
                        })
                    })
                }),
            Self::WrittenBook(predicate) => stack
                .component(vanilla_components::WRITTEN_BOOK_CONTENT)
                .is_some_and(|book| {
                    predicate
                        .author()
                        .is_none_or(|author| author == book.author())
                        && predicate
                            .title()
                            .is_none_or(|title| title == book.title().raw())
                        && int_bounds_matches(predicate.generation(), book.generation())
                        && predicate
                            .resolved()
                            .is_none_or(|resolved| resolved == book.resolved())
                        && predicate.pages().is_none_or(|pages| {
                            collection_matches(pages, book.pages().iter(), |predicate, page| {
                                predicate.contents() == page.raw()
                            })
                        })
                }),
            Self::Trim(predicate) => {
                stack
                    .component(vanilla_components::TRIM)
                    .is_some_and(|trim| {
                        predicate.material().is_none_or(|materials| {
                            trim.material()
                                .as_reference()
                                .is_some_and(|material| materials.contains(material))
                        }) && predicate.pattern().is_none_or(|patterns| {
                            trim.pattern()
                                .as_reference()
                                .is_some_and(|pattern| patterns.contains(pattern))
                        })
                    })
            }
            Self::JukeboxPlayable(predicate) => stack
                .component(vanilla_components::JUKEBOX_PLAYABLE)
                .is_some_and(|playable| {
                    predicate.song().is_none_or(|songs| {
                        playable
                            .song()
                            .as_reference()
                            .is_some_and(|song| songs.contains(song))
                    })
                }),
            Self::VillagerType(predicate) => stack
                .component(vanilla_components::VILLAGER_VARIANT)
                .is_some_and(|villager_type| {
                    predicate.villager_types().contains(villager_type.value())
                }),
        }
    }
}

fn int_bounds_matches(bounds: IntBounds, value: i32) -> bool {
    bounds.min().is_none_or(|minimum| value >= minimum)
        && bounds.max().is_none_or(|maximum| value <= maximum)
}

fn double_bounds_matches(bounds: DoubleBounds, value: f64) -> bool {
    bounds.min().is_none_or(|minimum| value >= minimum)
        && bounds.max().is_none_or(|maximum| value <= maximum)
}

fn collection_matches<'a, P, T: 'a>(
    predicate: &registered::CollectionPredicate<P>,
    values: impl IntoIterator<Item = &'a T>,
    matches: impl Fn(&P, &T) -> bool + Copy,
) -> bool {
    let values = values.into_iter().collect::<Vec<_>>();
    predicate.contains().is_none_or(|predicates| {
        predicates
            .iter()
            .all(|predicate| values.iter().any(|value| matches(predicate, value)))
    }) && predicate.counts().is_none_or(|predicates| {
        predicates.iter().all(|predicate| {
            let count = values
                .iter()
                .filter(|value| matches(predicate.test(), value))
                .count();
            i32::try_from(count).is_ok_and(|count| int_bounds_matches(predicate.count(), count))
        })
    }) && predicate.size().is_none_or(|size| {
        i32::try_from(values.len()).is_ok_and(|length| int_bounds_matches(*size, length))
    })
}

pub(crate) fn registered_item_predicate_matches<S: ItemInstanceView + ?Sized>(
    predicate: &RegisteredItemPredicate,
    template: &S,
) -> bool {
    predicate
        .items()
        .is_none_or(|items| items.contains(template.item_ref()))
        && int_bounds_matches(predicate.count(), template.item_count())
        && predicate
            .components()
            .exact()
            .values()
            .iter()
            .all(|(entry, expected)| template.effective_value_raw(&entry.key) == Some(expected))
        && predicate
            .components()
            .partial()
            .iter()
            .all(|predicate| registered_partial_matches(predicate, template))
}

fn registered_partial_matches<S: ItemInstanceView + ?Sized>(
    predicate: &registered::DataComponentPredicateData,
    stack: &S,
) -> bool {
    if let Some(component) = predicate.any_component() {
        return stack.effective_value_raw(&component.key).is_some();
    }
    if let Some(value) = predicate.downcast_ref::<registered::PotionsPredicate>() {
        return DataComponentPredicate::Potions(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::ContainerPredicate>() {
        return DataComponentPredicate::Container(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::BundlePredicate>() {
        return DataComponentPredicate::Bundle(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::FireworkExplosionPredicate>() {
        return DataComponentPredicate::FireworkExplosion(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::FireworksPredicate>() {
        return DataComponentPredicate::Fireworks(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::WritableBookPredicate>() {
        return DataComponentPredicate::WritableBook(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::WrittenBookPredicate>() {
        return DataComponentPredicate::WrittenBook(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::TrimPredicate>() {
        return DataComponentPredicate::Trim(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::JukeboxPlayablePredicate>() {
        return DataComponentPredicate::JukeboxPlayable(value.clone()).matches(stack);
    }
    if let Some(value) = predicate.downcast_ref::<registered::VillagerTypePredicate>() {
        return DataComponentPredicate::VillagerType(value.clone()).matches(stack);
    }

    if let Some(value) = predicate.downcast_ref::<registered::CustomDataPredicate>() {
        return stack
            .component(vanilla_components::CUSTOM_DATA)
            .cloned()
            .unwrap_or_default()
            .matched_by(value.value().tag());
    }
    if let Some(value) = predicate.downcast_ref::<registered::DamagePredicate>() {
        let Some(damage) = stack.component(vanilla_components::DAMAGE).copied() else {
            return false;
        };
        let maximum = stack
            .component(vanilla_components::MAX_DAMAGE)
            .copied()
            .unwrap_or(0);
        return int_bounds_matches(value.durability(), maximum - damage)
            && int_bounds_matches(value.damage(), damage);
    }
    if let Some(value) = predicate.downcast_ref::<registered::EnchantmentsPredicate>() {
        return stack
            .component(vanilla_components::ENCHANTMENTS)
            .is_some_and(|enchantments| {
                value
                    .enchantments()
                    .iter()
                    .all(|predicate| registered_enchantment_matches(predicate, enchantments))
            });
    }
    if let Some(value) = predicate.downcast_ref::<registered::StoredEnchantmentsPredicate>() {
        return stack
            .component(vanilla_components::STORED_ENCHANTMENTS)
            .is_some_and(|enchantments| {
                value
                    .enchantments()
                    .iter()
                    .all(|predicate| registered_enchantment_matches(predicate, enchantments))
            });
    }
    if let Some(value) = predicate.downcast_ref::<registered::AttributeModifiersPredicate>() {
        return stack
            .component(vanilla_components::ATTRIBUTE_MODIFIERS)
            .is_some_and(|modifiers| {
                value.modifiers().is_none_or(|predicate| {
                    collection_matches(
                        predicate,
                        modifiers.modifiers.iter(),
                        registered_attribute_modifier_matches,
                    )
                })
            });
    }
    false
}

fn registered_enchantment_matches(
    predicate: &registered::EnchantmentPredicate,
    enchantments: &vanilla_components::ItemEnchantments,
) -> bool {
    enchantments.iter().any(|(key, level)| {
        predicate.enchantments().is_none_or(|expected| {
            REGISTRY
                .enchantments
                .by_key(key)
                .is_some_and(|enchantment| expected.contains(enchantment))
        }) && predicate
            .levels()
            .min()
            .is_none_or(|minimum| i64::from(*level) >= i64::from(minimum))
            && predicate
                .levels()
                .max()
                .is_none_or(|maximum| i64::from(*level) <= i64::from(maximum))
    })
}

fn registered_attribute_modifier_matches(
    predicate: &registered::AttributeModifierEntryPredicate,
    modifier: &vanilla_components::ItemAttributeModifierEntry,
) -> bool {
    predicate
        .attribute()
        .is_none_or(|attributes| attributes.contains(modifier.attribute))
        && predicate.id().is_none_or(|id| id == &modifier.id)
        && double_bounds_matches(predicate.amount(), modifier.amount)
        && predicate
            .operation()
            .is_none_or(|operation| operation == modifier.operation)
        && predicate.slot().is_none_or(|slot| slot == modifier.slot)
}

fn firework_matches(
    predicate: &registered::FireworkPredicate,
    explosion: &vanilla_components::FireworkExplosion,
) -> bool {
    predicate
        .shape()
        .is_none_or(|shape| shape == explosion.shape())
        && predicate
            .has_twinkle()
            .is_none_or(|twinkle| twinkle == explosion.has_twinkle())
        && predicate
            .has_trail()
            .is_none_or(|trail| trail == explosion.has_trail())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct IntRange {
    pub(crate) min: Option<i32>,
    pub(crate) max: Option<i32>,
}

impl IntRange {
    pub(crate) const ANY: Self = Self {
        min: None,
        max: None,
    };

    pub(crate) fn matches(self, value: i32) -> bool {
        self.min.is_none_or(|minimum| value >= minimum)
            && self.max.is_none_or(|maximum| value <= maximum)
    }

    pub(crate) fn matches_u32(self, value: u32) -> bool {
        let value = i64::from(value);
        self.min.is_none_or(|minimum| value >= i64::from(minimum))
            && self.max.is_none_or(|maximum| value <= i64::from(maximum))
    }

    pub(crate) fn matches_usize(self, value: usize) -> bool {
        let minimum_matches = self.min.is_none_or(|minimum| {
            minimum <= 0 || usize::try_from(minimum).is_ok_and(|minimum| value >= minimum)
        });
        let maximum_matches = self
            .max
            .is_none_or(|maximum| usize::try_from(maximum).is_ok_and(|maximum| value <= maximum));
        minimum_matches && maximum_matches
    }

    pub(crate) const fn is_any(self) -> bool {
        self.min.is_none() && self.max.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DamagePredicate {
    pub(crate) durability: IntRange,
    pub(crate) damage: IntRange,
}

impl DamagePredicate {
    pub(crate) fn matches<S: ItemInstanceView + ?Sized>(self, stack: &S) -> bool {
        let Some(damage) = stack.component(vanilla_components::DAMAGE).copied() else {
            return false;
        };
        let maximum = stack
            .component(vanilla_components::MAX_DAMAGE)
            .copied()
            .unwrap_or(0);
        self.durability.matches(maximum - damage) && self.damage.matches(damage)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EnchantmentPredicate {
    pub(crate) enchantments: Option<Vec<EnchantmentRef>>,
    pub(crate) levels: IntRange,
}

impl EnchantmentPredicate {
    pub(crate) fn matches(&self, enchantments: &vanilla_components::ItemEnchantments) -> bool {
        if let Some(expected) = &self.enchantments {
            return expected.iter().any(|enchantment| {
                let level = enchantments.get_level(&enchantment.key);
                level != 0 && self.levels.matches_u32(level)
            });
        }
        if !self.levels.is_any() {
            return enchantments
                .iter()
                .any(|(_, level)| self.levels.matches_u32(*level));
        }
        !enchantments.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttributeModifiersPredicate {
    pub(crate) modifiers: Option<AttributeModifierCollectionPredicate>,
}

impl AttributeModifiersPredicate {
    pub(crate) fn matches(&self, modifiers: &vanilla_components::ItemAttributeModifiers) -> bool {
        self.modifiers
            .as_ref()
            .is_none_or(|predicate| predicate.matches(&modifiers.modifiers))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttributeModifierCollectionPredicate {
    pub(crate) contains: Vec<AttributeModifierEntryPredicate>,
    pub(crate) counts: Vec<AttributeModifierCountPredicate>,
    pub(crate) size: Option<IntRange>,
}

impl AttributeModifierCollectionPredicate {
    pub(crate) fn matches(
        &self,
        modifiers: &[vanilla_components::ItemAttributeModifierEntry],
    ) -> bool {
        let mut matched = vec![false; self.contains.len()];
        for modifier in modifiers {
            for (matched, predicate) in matched.iter_mut().zip(&self.contains) {
                if !*matched && predicate.matches(modifier) {
                    *matched = true;
                }
            }
        }
        matched.into_iter().all(|matched| matched)
            && self.counts.iter().all(|predicate| {
                let count = modifiers
                    .iter()
                    .filter(|modifier| predicate.test.matches(modifier))
                    .count();
                predicate.count.matches_usize(count)
            })
            && self
                .size
                .is_none_or(|range| range.matches_usize(modifiers.len()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttributeModifierCountPredicate {
    pub(crate) test: AttributeModifierEntryPredicate,
    pub(crate) count: IntRange,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttributeModifierEntryPredicate {
    pub(crate) attributes: Option<Vec<AttributeRef>>,
    pub(crate) id: Option<Identifier>,
    pub(crate) amount: DoubleRange,
    pub(crate) operation: Option<AttributeModifierOperation>,
    pub(crate) slot: Option<EquipmentSlotGroup>,
}

impl AttributeModifierEntryPredicate {
    pub(crate) fn matches(
        &self,
        modifier: &vanilla_components::ItemAttributeModifierEntry,
    ) -> bool {
        self.attributes.as_ref().is_none_or(|attributes| {
            attributes
                .iter()
                .any(|attribute| attribute.key == modifier.attribute.key)
        }) && self.id.as_ref().is_none_or(|id| id == &modifier.id)
            && self.amount.matches(modifier.amount)
            && self
                .operation
                .is_none_or(|operation| operation == modifier.operation)
            && self.slot.is_none_or(|slot| slot == modifier.slot)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DoubleRange {
    pub(crate) min: Option<f64>,
    pub(crate) max: Option<f64>,
}

impl DoubleRange {
    pub(crate) const ANY: Self = Self {
        min: None,
        max: None,
    };

    pub(crate) fn matches(self, value: f64) -> bool {
        self.min
            .is_none_or(|minimum| minimum.partial_cmp(&value) != Some(Ordering::Greater))
            && self
                .max
                .is_none_or(|maximum| maximum.partial_cmp(&value) != Some(Ordering::Less))
    }
}
