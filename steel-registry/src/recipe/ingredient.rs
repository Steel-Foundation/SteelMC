//! Ingredient matching for recipes.

use steel_utils::Identifier;

use crate::{REGISTRY, item_stack::ItemStack, items::ItemRef};

/// Represents what items can satisfy a recipe slot.
/// Matches Java's `Ingredient` class.
#[derive(Debug, Clone)]
pub enum Ingredient {
    /// Matches nothing (empty slot required).
    Empty,
    /// Matches a single specific item.
    Item(ItemRef),
    /// Matches any item in a tag.
    Tag(Identifier),
    /// Matches any of the listed items (OR).
    Choice(&'static [ItemRef]),
}

impl Ingredient {
    /// Tests if the given item stack satisfies this ingredient.
    #[must_use]
    pub fn test(&self, stack: &ItemStack) -> bool {
        match self {
            Self::Empty => stack.is_empty(),
            Self::Item(item) => !stack.is_empty() && std::ptr::eq(*item, stack.item),
            Self::Tag(tag) => {
                if stack.is_empty() {
                    return false;
                }
                REGISTRY.items.is_in_tag(stack.item, tag)
            }
            Self::Choice(items) => {
                if stack.is_empty() {
                    return false;
                }
                items.iter().any(|&item| std::ptr::eq(item, stack.item))
            }
        }
    }

    /// Returns true if this ingredient matches an empty slot.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns all items that could satisfy this ingredient (for recipe book display).
    #[must_use]
    pub fn get_items(&self) -> Vec<ItemRef> {
        match self {
            Self::Empty => Vec::new(),
            Self::Item(item) => vec![*item],
            Self::Tag(tag) => REGISTRY
                .items
                .get_tag(tag)
                .map(|items| items.to_vec())
                .unwrap_or_default(),
            Self::Choice(items) => items.to_vec(),
        }
    }

    /// Compares two ingredients for equality (used for symmetry detection).
    #[must_use]
    pub fn eq_ingredient(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Item(a), Self::Item(b)) => std::ptr::eq(*a, *b),
            (Self::Tag(a), Self::Tag(b)) => a == b,
            (Self::Choice(a), Self::Choice(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| std::ptr::eq(*x, *y))
            }
            _ => false,
        }
    }
}
