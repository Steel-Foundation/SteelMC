//! Vanilla crafting recipe matching implementations.

use crate::data_components::vanilla_components::{
    BANNER_PATTERNS, DAMAGE, DYE, MAX_DAMAGE, WRITTEN_BOOK_CONTENT,
};
use crate::item_stack::ItemStack;

use super::{
    BannerDuplicateRecipe, BookCloningRecipe, CraftingInput, CraftingRecipe, DecoratedPotRecipe,
    DyeRecipe, FireworkRocketRecipe, FireworkStarFadeRecipe, FireworkStarRecipe, ImbueRecipe,
    Ingredient, MapExtendingRecipe, RepairItemRecipe, ShapedRecipe, ShapelessRecipe,
    ShieldDecorationRecipe, TransmuteRecipe,
};

pub(crate) fn matches(recipe: &CraftingRecipe, input: &CraftingInput) -> bool {
    match recipe {
        CraftingRecipe::Shaped(recipe) => shaped(recipe, input),
        CraftingRecipe::Shapeless(recipe) => shapeless(recipe, input),
        CraftingRecipe::Transmute(recipe) => transmute(recipe, input),
        CraftingRecipe::Dye(recipe) => dye(recipe, input),
        CraftingRecipe::DecoratedPot(recipe) => decorated_pot(recipe, input),
        CraftingRecipe::Imbue(recipe) => imbue(recipe, input),
        CraftingRecipe::BannerDuplicate(recipe) => banner_duplicate(recipe, input),
        CraftingRecipe::BookCloning(recipe) => book_cloning(recipe, input),
        CraftingRecipe::FireworkRocket(recipe) => firework_rocket(recipe, input),
        CraftingRecipe::FireworkStar(recipe) => firework_star(recipe, input),
        CraftingRecipe::FireworkStarFade(recipe) => firework_star_fade(recipe, input),
        CraftingRecipe::MapExtending(recipe) => map_extending(recipe, input),
        CraftingRecipe::RepairItem(recipe) => repair_item(recipe, input),
        CraftingRecipe::ShieldDecoration(recipe) => shield_decoration(recipe, input),
    }
}

fn shaped(recipe: &ShapedRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count()
        != recipe
            .pattern
            .iter()
            .filter(|ingredient| !ingredient.is_empty())
            .count()
        || input.width != recipe.width
        || input.height != recipe.height
    {
        return false;
    }
    matches_shaped_orientation(recipe, input, false)
        || (!recipe.symmetrical && matches_shaped_orientation(recipe, input, true))
}

fn matches_shaped_orientation(
    recipe: &ShapedRecipe,
    input: &CraftingInput,
    mirrored: bool,
) -> bool {
    for y in 0..recipe.height {
        for x in 0..recipe.width {
            let pattern_x = if mirrored { recipe.width - 1 - x } else { x };
            if !recipe.pattern[y * recipe.width + pattern_x].test(input.get(x, y)) {
                return false;
            }
        }
    }
    true
}

fn shapeless(recipe: &ShapelessRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() != recipe.ingredients.len() {
        return false;
    }
    let items: Vec<_> = input
        .items
        .iter()
        .filter(|stack| !stack.is_empty())
        .collect();
    let mut used = vec![false; items.len()];
    match_shapeless_ingredient(&recipe.ingredients, &items, &mut used, 0)
}

fn match_shapeless_ingredient(
    ingredients: &[Ingredient],
    items: &[&ItemStack],
    used: &mut [bool],
    ingredient_index: usize,
) -> bool {
    if ingredient_index == ingredients.len() {
        return true;
    }
    for item_index in 0..items.len() {
        if used[item_index] || !ingredients[ingredient_index].test(items[item_index]) {
            continue;
        }
        used[item_index] = true;
        if match_shapeless_ingredient(ingredients, items, used, ingredient_index + 1) {
            return true;
        }
        used[item_index] = false;
    }
    false
}

fn transmute(recipe: &TransmuteRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() < recipe.min_material_count + 1
        || input.ingredient_count() > recipe.max_material_count + 1
    {
        return false;
    }
    let mut found_input = None;
    let mut material_count = 0;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.input.test(stack) {
            if found_input.is_some() {
                return false;
            }
            found_input = Some(stack);
        } else if recipe.material.test(stack) {
            material_count += 1;
            if material_count > recipe.max_material_count {
                return false;
            }
        } else {
            return false;
        }
    }
    let Some(found_input) = found_input else {
        return false;
    };
    if !(recipe.min_material_count..=recipe.max_material_count).contains(&material_count) {
        return false;
    }
    let result_count = if recipe.add_material_count_to_result {
        recipe.result.count() + i32::try_from(material_count).unwrap_or(i32::MAX)
    } else {
        recipe.result.count()
    };
    if result_count != 1 {
        return true;
    }
    let result = recipe.result.apply(1, found_input.components_patch());
    !result.is_empty() && !ItemStack::is_same_item_same_components(found_input, &result)
}

fn dye(recipe: &DyeRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_target = false;
    let mut has_dye = false;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.target.test(stack) {
            if has_target {
                return false;
            }
            has_target = true;
        } else if recipe.dye.test(stack) && stack.has(DYE) {
            has_dye = true;
        } else {
            return false;
        }
    }
    has_target && has_dye
}

fn decorated_pot(recipe: &DecoratedPotRecipe, input: &CraftingInput) -> bool {
    input.width == 3
        && input.height == 3
        && input.ingredient_count() == 4
        && recipe.back.test(input.get(1, 0))
        && recipe.left.test(input.get(0, 1))
        && recipe.right.test(input.get(2, 1))
        && recipe.front.test(input.get(1, 2))
}

fn imbue(recipe: &ImbueRecipe, input: &CraftingInput) -> bool {
    if input.width != 3 || input.height != 3 || input.ingredient_count() != 9 {
        return false;
    }
    for y in 0..3 {
        for x in 0..3 {
            let ingredient = if x == 1 && y == 1 {
                &recipe.source
            } else {
                &recipe.material
            };
            if !ingredient.test(input.get(x, y)) {
                return false;
            }
        }
    }
    true
}

fn banner_duplicate(recipe: &BannerDuplicateRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() != 2 {
        return false;
    }
    let mut has_target = false;
    let mut has_source = false;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if !recipe.banner.test(stack) {
            return false;
        }
        let pattern_count = stack
            .get(BANNER_PATTERNS)
            .map_or(0, |patterns| patterns.layers().len());
        if pattern_count > 6 {
            return false;
        }
        if pattern_count > 0 {
            if has_source {
                return false;
            }
            has_source = true;
        } else {
            if has_target {
                return false;
            }
            has_target = true;
        }
    }
    has_source && has_target
}

fn book_cloning(recipe: &BookCloningRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_source = false;
    let mut has_material = false;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.source.test(stack) {
            let Some(content) = stack.get(WRITTEN_BOOK_CONTENT) else {
                return false;
            };
            if has_source
                || !(recipe.min_generation..=recipe.max_generation).contains(&content.generation())
            {
                return false;
            }
            has_source = true;
        } else if recipe.material.test(stack) {
            has_material = true;
        } else {
            return false;
        }
    }
    has_source && has_material
}

fn firework_rocket(recipe: &FireworkRocketRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_shell = false;
    let mut fuel_count = 0;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.shell.test(stack) {
            if has_shell {
                return false;
            }
            has_shell = true;
        } else if recipe.fuel.test(stack) {
            fuel_count += 1;
            if fuel_count > 3 {
                return false;
            }
        } else if !recipe.star.test(stack) {
            return false;
        }
    }
    has_shell && fuel_count >= 1
}

fn firework_star(recipe: &FireworkStarRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_fuel = false;
    let mut has_dye = false;
    let mut has_shape = false;
    let mut has_trail = false;
    let mut has_twinkle = false;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.twinkle.test(stack) {
            if has_twinkle {
                return false;
            }
            has_twinkle = true;
        } else if recipe.trail.test(stack) {
            if has_trail {
                return false;
            }
            has_trail = true;
        } else if recipe.fuel.test(stack) {
            if has_fuel {
                return false;
            }
            has_fuel = true;
        } else if recipe.dye.test(stack) && stack.has(DYE) {
            has_dye = true;
        } else if recipe
            .shapes
            .iter()
            .any(|(_, ingredient)| ingredient.test(stack))
        {
            if has_shape {
                return false;
            }
            has_shape = true;
        } else {
            return false;
        }
    }
    has_fuel && has_dye
}

fn firework_star_fade(recipe: &FireworkStarFadeRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() < 2 {
        return false;
    }
    let mut has_target = false;
    let mut has_dye = false;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.dye.test(stack) && stack.has(DYE) {
            has_dye = true;
        } else if recipe.target.test(stack) {
            if has_target {
                return false;
            }
            has_target = true;
        } else {
            return false;
        }
    }
    has_target && has_dye
}

fn map_extending(recipe: &MapExtendingRecipe, input: &CraftingInput) -> bool {
    if input.width != 3 || input.height != 3 || input.ingredient_count() != 9 {
        return false;
    }
    for y in 0..3 {
        for x in 0..3 {
            let ingredient = if x == 1 && y == 1 {
                &recipe.map
            } else {
                &recipe.material
            };
            if !ingredient.test(input.get(x, y)) {
                return false;
            }
        }
    }
    let Some(data) = input.map_data(4) else {
        return false;
    };
    !data.exploration_map && data.scale < 4
}

fn repair_item(_recipe: &RepairItemRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() != 2 {
        return false;
    }
    let mut stacks = input.items.iter().filter(|stack| !stack.is_empty());
    let Some(first) = stacks.next() else {
        return false;
    };
    let Some(second) = stacks.next() else {
        return false;
    };
    first.item() == second.item()
        && first.count() == 1
        && second.count() == 1
        && first.has(MAX_DAMAGE)
        && second.has(MAX_DAMAGE)
        && first.has(DAMAGE)
        && second.has(DAMAGE)
}

fn shield_decoration(recipe: &ShieldDecorationRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() != 2 {
        return false;
    }
    let mut has_banner = false;
    let mut has_target = false;
    for stack in input.items.iter().filter(|stack| !stack.is_empty()) {
        if recipe.banner.test(stack) {
            if has_banner {
                return false;
            }
            has_banner = true;
        } else if recipe.target.test(stack) {
            if has_target
                || stack
                    .get(BANNER_PATTERNS)
                    .is_some_and(|patterns| !patterns.layers().is_empty())
            {
                return false;
            }
            has_target = true;
        } else {
            return false;
        }
    }
    has_banner && has_target
}
