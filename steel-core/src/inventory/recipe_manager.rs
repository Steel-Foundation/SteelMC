//! Recipe matching and crafting-grid interpretation.

use steel_registry::data_components::vanilla_components::{
    BANNER_PATTERNS, BASE_COLOR, DYE, DYED_COLOR, ENCHANTMENTS, FIREWORK_EXPLOSION, FIREWORKS,
    MAP_POST_PROCESSING, MAX_DAMAGE, POT_DECORATIONS, POTION_CONTENTS, WRITTEN_BOOK_CONTENT,
};
use steel_registry::data_components::vanilla_components::{
    DyedItemColor, FireworkExplosion, FireworkExplosionShape, Fireworks, ItemEnchantments,
    MapPostProcessing, PotDecorations,
};
use steel_registry::recipe::{
    BookCloningRecipe, CraftingInput, CraftingRecipe, DyeRecipe, FireworkRocketRecipe,
    FireworkStarFadeRecipe, FireworkStarRecipe, PositionedCraftingInput, ShieldDecorationRecipe,
    TypedRecipeRef, vanilla_recipe_types,
};
use steel_registry::{
    DyeColor, REGISTRY, RegistryExt as _, TaggedRegistryExt as _, item_stack::ItemStack,
    vanilla_enchantment_tags, vanilla_items,
};

use crate::inventory::container::CraftingContainer;

use super::container::Container;

/// Typed reference to one registered crafting recipe.
pub type CraftingRecipeRef = TypedRecipeRef<CraftingRecipe, CraftingInput>;

/// Updates a result container from the first matching crafting recipe.
pub fn slot_changed_crafting_grid<R: Container>(crafting: &CraftingContainer, result: &mut R) {
    result.set_item(
        0,
        assemble_for_container(crafting).unwrap_or_else(ItemStack::empty),
    );
}

/// Finds the first crafting recipe in deterministic registry order.
#[must_use]
pub fn find_recipe(crafting: &CraftingContainer) -> Option<CraftingRecipeRef> {
    let positioned = crafting.as_positioned_input();
    REGISTRY
        .recipes
        .find_match(&vanilla_recipe_types::CRAFTING, &positioned.input)
}

/// Finds and interprets the output for the current crafting container.
#[must_use]
pub fn assemble_for_container(crafting: &CraftingContainer) -> Option<ItemStack> {
    let positioned = crafting.as_positioned_input();
    let recipe = find_recipe(crafting)?;
    Some(assemble_recipe(recipe, &positioned.input))
}

/// Interprets passive vanilla crafting data for a concrete input snapshot.
#[must_use]
pub fn assemble_recipe(recipe: CraftingRecipeRef, input: &CraftingInput) -> ItemStack {
    match recipe.data() {
        CraftingRecipe::Shaped(recipe) => recipe.result.create(),
        CraftingRecipe::Shapeless(recipe) => recipe.result.create(),
        CraftingRecipe::Transmute(recipe) => {
            let material_count = input
                .items
                .iter()
                .filter(|stack| !stack.is_empty() && recipe.material.test(stack))
                .count();
            let extra_count = if recipe.add_material_count_to_result {
                i32::try_from(material_count).unwrap_or(i32::MAX)
            } else {
                0
            };
            input
                .items
                .iter()
                .find(|stack| !stack.is_empty() && recipe.input.test(stack))
                .map_or_else(ItemStack::empty, |source| {
                    recipe.result.apply(
                        recipe.result.count().saturating_add(extra_count),
                        source.components_patch(),
                    )
                })
        }
        CraftingRecipe::Dye(recipe) => assemble_dye(recipe, input),
        CraftingRecipe::DecoratedPot(recipe) => {
            let decorations = [
                input.get(1, 0).item(),
                input.get(0, 1).item(),
                input.get(2, 1).item(),
                input.get(1, 2).item(),
            ];
            let Ok(decorations) = PotDecorations::from_ordered(&decorations) else {
                return ItemStack::empty();
            };
            let mut result = recipe.result.create();
            result.set(POT_DECORATIONS, decorations);
            result
        }
        CraftingRecipe::Imbue(recipe) => {
            let mut result = recipe.result.create();
            if let Some(contents) = input.get(1, 1).get(POTION_CONTENTS) {
                result.set(POTION_CONTENTS, contents.clone());
            }
            result
        }
        CraftingRecipe::BannerDuplicate(recipe) => input
            .items
            .iter()
            .find(|stack| {
                recipe.banner.test(stack)
                    && stack
                        .get(BANNER_PATTERNS)
                        .is_some_and(|patterns| !patterns.layers().is_empty())
            })
            .map_or_else(ItemStack::empty, |source| {
                recipe
                    .result
                    .apply(recipe.result.count(), source.components_patch())
            }),
        CraftingRecipe::BookCloning(recipe) => assemble_book_cloning(recipe, input),
        CraftingRecipe::FireworkRocket(recipe) => assemble_firework_rocket(recipe, input),
        CraftingRecipe::FireworkStar(recipe) => assemble_firework_star(recipe, input),
        CraftingRecipe::FireworkStarFade(recipe) => assemble_firework_fade(recipe, input),
        CraftingRecipe::MapExtending(recipe) => {
            let source = input.get(1, 1);
            let mut result = recipe
                .result
                .apply(recipe.result.count(), source.components_patch());
            result.set(MAP_POST_PROCESSING, MapPostProcessing::Scale);
            result
        }
        CraftingRecipe::RepairItem(_) => assemble_repair(input),
        CraftingRecipe::ShieldDecoration(recipe) => assemble_shield(recipe, input),
    }
}

/// Returns crafting remainders and the positioned input used to map them back
/// to the original grid.
#[must_use]
pub fn get_remaining_items(
    crafting: &CraftingContainer,
) -> Option<(Vec<ItemStack>, PositionedCraftingInput)> {
    let positioned = crafting.as_positioned_input();
    let recipe = find_recipe(crafting)?;
    let mut remainders: Vec<_> = positioned
        .input
        .items
        .iter()
        .map(|stack| {
            if stack.is_empty() {
                ItemStack::empty()
            } else {
                stack.item().get_crafting_remainder()
            }
        })
        .collect();

    match recipe.data() {
        CraftingRecipe::BannerDuplicate(_) => {
            for (slot, stack) in positioned.input.items.iter().enumerate() {
                if remainders[slot].is_empty()
                    && stack
                        .get(BANNER_PATTERNS)
                        .is_some_and(|patterns| !patterns.layers().is_empty())
                {
                    remainders[slot] = stack.copy_with_count(1);
                }
            }
        }
        CraftingRecipe::BookCloning(_) => {
            for (slot, stack) in positioned.input.items.iter().enumerate() {
                if remainders[slot].is_empty() && stack.has(WRITTEN_BOOK_CONTENT) {
                    remainders[slot] = stack.copy_with_count(1);
                    break;
                }
            }
        }
        _ => {}
    }

    Some((remainders, positioned))
}

fn assemble_dye(recipe: &DyeRecipe, input: &CraftingInput) -> ItemStack {
    let mut target = None;
    let mut dyes = Vec::new();
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.target.test(stack) {
            target = Some(stack);
        } else if recipe.dye.test(stack) {
            dyes.push(stack.get(DYE).copied().unwrap_or(DyeColor::White));
        }
    }
    let Some(target) = target else {
        return ItemStack::empty();
    };
    if dyes.is_empty() {
        return ItemStack::empty();
    }
    let color = DyedItemColor::apply_dyes(target.get(DYED_COLOR).copied(), &dyes);
    let mut result = recipe
        .result
        .apply(recipe.result.count(), target.components_patch());
    result.set(DYED_COLOR, color);
    result
}

fn assemble_book_cloning(recipe: &BookCloningRecipe, input: &CraftingInput) -> ItemStack {
    let Some(source) = input
        .items
        .iter()
        .find(|stack| !stack.is_empty() && recipe.source.test(stack))
    else {
        return ItemStack::empty();
    };
    let Some(content) = source.get(WRITTEN_BOOK_CONTENT) else {
        return ItemStack::empty();
    };
    let material_count = input
        .items
        .iter()
        .filter(|stack| !stack.is_empty() && recipe.material.test(stack))
        .count();
    let Ok(extra_count) = i32::try_from(material_count.saturating_sub(1)) else {
        return ItemStack::empty();
    };
    let mut result = recipe.result.apply(
        recipe.result.count().saturating_add(extra_count),
        source.components_patch(),
    );
    result.set(WRITTEN_BOOK_CONTENT, content.craft_copy());
    result
}

fn assemble_firework_rocket(recipe: &FireworkRocketRecipe, input: &CraftingInput) -> ItemStack {
    let mut flight_duration = 0;
    let mut explosions = Vec::new();
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.fuel.test(stack) {
            flight_duration += 1;
        } else if recipe.star.test(stack)
            && let Some(explosion) = stack.get(FIREWORK_EXPLOSION)
        {
            explosions.push(explosion.clone());
        }
    }
    let Ok(fireworks) = Fireworks::new(flight_duration, explosions) else {
        return ItemStack::empty();
    };
    let mut result = recipe.result.create();
    result.set(FIREWORKS, fireworks);
    result
}

fn assemble_firework_star(recipe: &FireworkStarRecipe, input: &CraftingInput) -> ItemStack {
    let mut shape = FireworkExplosionShape::SmallBall;
    let mut has_twinkle = false;
    let mut has_trail = false;
    let mut colors = Vec::new();
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if let Some((found_shape, _)) = recipe
            .shapes
            .iter()
            .find(|(_, ingredient)| ingredient.test(stack))
        {
            shape = *found_shape;
        } else if recipe.twinkle.test(stack) {
            has_twinkle = true;
        } else if recipe.trail.test(stack) {
            has_trail = true;
        } else if recipe.dye.test(stack) {
            colors.push(
                stack
                    .get(DYE)
                    .copied()
                    .unwrap_or(DyeColor::White)
                    .firework_color(),
            );
        }
    }
    let mut result = recipe.result.create();
    result.set(
        FIREWORK_EXPLOSION,
        FireworkExplosion::new(shape, colors, Vec::new(), has_trail, has_twinkle),
    );
    result
}

fn assemble_firework_fade(recipe: &FireworkStarFadeRecipe, input: &CraftingInput) -> ItemStack {
    let Some(target) = input
        .items
        .iter()
        .find(|stack| !stack.is_empty() && recipe.target.test(stack))
    else {
        return ItemStack::empty();
    };
    let fade_colors: Vec<_> = input
        .items
        .iter()
        .filter(|stack| !stack.is_empty() && recipe.dye.test(stack))
        .map(|stack| {
            stack
                .get(DYE)
                .copied()
                .unwrap_or(DyeColor::White)
                .firework_color()
        })
        .collect();
    let mut result = recipe
        .result
        .apply(recipe.result.count(), target.components_patch());
    let explosion = target
        .get(FIREWORK_EXPLOSION)
        .cloned()
        .unwrap_or_default()
        .with_fade_colors(fade_colors);
    result.set(FIREWORK_EXPLOSION, explosion);
    result
}

fn assemble_repair(input: &CraftingInput) -> ItemStack {
    let mut inputs = input.items.iter().filter(|stack| !stack.is_empty());
    let Some(first) = inputs.next() else {
        return ItemStack::empty();
    };
    let Some(second) = inputs.next() else {
        return ItemStack::empty();
    };
    let durability = first.get_max_damage().max(second.get_max_damage());
    let remaining = (first.get_max_damage() - first.get_damage_value())
        + (second.get_max_damage() - second.get_damage_value())
        + durability * 5 / 100;
    let mut result = ItemStack::new(first.item());
    result.set(MAX_DAMAGE, durability);
    result.set_damage_value((durability - remaining).max(0));

    let mut curses = ItemEnchantments::empty();
    for enchantments in [
        first.get_enchantments_for_crafting(),
        second.get_enchantments_for_crafting(),
    ]
    .into_iter()
    .flatten()
    {
        for (key, level) in enchantments.iter() {
            let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
                continue;
            };
            if REGISTRY.enchantments.is_in_tag(
                enchantment,
                &vanilla_enchantment_tags::EnchantmentTag::CURSE,
            ) {
                curses.upgrade(key.clone(), *level);
            }
        }
    }
    result.set(ENCHANTMENTS, curses);
    result
}

fn assemble_shield(recipe: &ShieldDecorationRecipe, input: &CraftingInput) -> ItemStack {
    let Some(banner) = input
        .items
        .iter()
        .find(|stack| !stack.is_empty() && recipe.banner.test(stack))
    else {
        return ItemStack::empty();
    };
    let Some(target) = input
        .items
        .iter()
        .find(|stack| !stack.is_empty() && recipe.target.test(stack))
    else {
        return ItemStack::empty();
    };
    let mut result = recipe
        .result
        .apply(recipe.result.count(), target.components_patch());
    if let Some(patterns) = banner.get(BANNER_PATTERNS) {
        result.set(BANNER_PATTERNS, patterns.clone());
    }
    result.set(BASE_COLOR, banner_color(banner));
    result
}

fn banner_color(stack: &ItemStack) -> DyeColor {
    let banners = [
        (&*vanilla_items::WHITE_BANNER, DyeColor::White),
        (&*vanilla_items::ORANGE_BANNER, DyeColor::Orange),
        (&*vanilla_items::MAGENTA_BANNER, DyeColor::Magenta),
        (&*vanilla_items::LIGHT_BLUE_BANNER, DyeColor::LightBlue),
        (&*vanilla_items::YELLOW_BANNER, DyeColor::Yellow),
        (&*vanilla_items::LIME_BANNER, DyeColor::Lime),
        (&*vanilla_items::PINK_BANNER, DyeColor::Pink),
        (&*vanilla_items::GRAY_BANNER, DyeColor::Gray),
        (&*vanilla_items::LIGHT_GRAY_BANNER, DyeColor::LightGray),
        (&*vanilla_items::CYAN_BANNER, DyeColor::Cyan),
        (&*vanilla_items::PURPLE_BANNER, DyeColor::Purple),
        (&*vanilla_items::BLUE_BANNER, DyeColor::Blue),
        (&*vanilla_items::BROWN_BANNER, DyeColor::Brown),
        (&*vanilla_items::GREEN_BANNER, DyeColor::Green),
        (&*vanilla_items::RED_BANNER, DyeColor::Red),
        (&*vanilla_items::BLACK_BANNER, DyeColor::Black),
    ];
    banners
        .into_iter()
        .find_map(|(item, color)| (stack.item() == item).then_some(color))
        .unwrap_or(DyeColor::White)
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::vanilla_components::DYE;
    use steel_registry::recipe::{CraftingInput, vanilla_recipe_types};
    use steel_registry::{DyeColor, REGISTRY, init_vanilla_registry, vanilla_items};

    use super::assemble_recipe;
    use steel_registry::item_stack::ItemStack;

    #[test]
    fn external_crafting_interpreter_assembles_a_generated_dye_recipe() {
        init_vanilla_registry();
        let input = CraftingInput::new(
            2,
            1,
            vec![
                ItemStack::new(&vanilla_items::RED_DYE),
                ItemStack::new(&vanilla_items::BLUE_DYE),
            ],
        );
        let Some(recipe) = REGISTRY
            .recipes
            .find_match(&vanilla_recipe_types::CRAFTING, &input)
        else {
            panic!("red and blue dye should match the purple dye recipe");
        };

        let result = assemble_recipe(recipe, &input);

        assert!(result.is(&vanilla_items::PURPLE_DYE));
        assert_eq!(result.get(DYE), Some(&DyeColor::Purple));
    }
}
