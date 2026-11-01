//! Shared operations used by compiled item modifier functions.
use steel_registry::recipe::{SingleItemRecipeInput, vanilla_recipe_types};
use steel_registry::{
    REGISTRY, data_components::vanilla_components::CUSTOM_NAME, item_stack::ItemStack,
};
use steel_utils::{
    Identifier,
    random::{Random, RandomSource},
};

/// The supported custom-name projection of a block entity's collected components.
pub(super) fn copy_block_entity_name(
    ctx: &super::LootContext<'_>,
    item: &mut ItemStack,
) -> super::LootResult<()> {
    let Some(block_entity) = ctx.block_entity else {
        return Ok(());
    };
    // TODO: Use collected block-entity components for custom-name copying,
    // including entities without Nameable and absent-name no-op behavior.
    let Some(nameable) = block_entity.as_nameable() else {
        return ctx.unsupported("copy_components/block_entity custom name");
    };
    if let Some(name) = nameable.custom_name() {
        item.set(CUSTOM_NAME, name);
    }
    Ok(())
}

pub(super) fn set_enchantment(item: &mut ItemStack, key: Identifier, level: i32, add: bool) {
    let Some(enchantments) = item.get_enchantments_for_crafting() else {
        return;
    };
    let base = if add {
        enchantments.get_level(&key) as i32
    } else {
        0
    };
    item.set_enchantments(
        &[(key, base.wrapping_add(level).clamp(0, 255) as u32)],
        false,
    );
}

pub(super) fn smelt(item: &mut ItemStack, use_input_count: bool) {
    if item.is_empty() {
        return;
    }
    let input = SingleItemRecipeInput::new(item.copy_with_count(item.count()));
    let Some(recipe) = REGISTRY
        .recipes
        .find_match(&vanilla_recipe_types::SMELTING, &input)
    else {
        log::warn!(
            "Couldn't smelt {} because there is no smelting recipe",
            item.item.key
        );
        return;
    };
    let mut result = recipe.data().result.create();
    if result.is_empty() {
        return;
    }
    let count = if use_input_count { item.count() } else { 1 };
    result.set_count(
        count
            .wrapping_mul(result.count())
            .min(result.max_stack_size()),
    );
    *item = result;
}

pub(super) fn uniform_bonus(
    item: &mut ItemStack,
    random: &mut RandomSource,
    level: i32,
    multiplier: i32,
) {
    let bonus = random.next_i32_bounded(multiplier * level + 1);
    item.set_count(item.count().wrapping_add(bonus));
}

// Widen before adding the half: an f32 addition would round the value directly
// below 0.5 up to 1.0. Java rounds negative ties toward positive infinity.
pub(super) fn round_count(value: f32) -> i32 {
    (f64::from(value) + 0.5).floor() as i32
}
