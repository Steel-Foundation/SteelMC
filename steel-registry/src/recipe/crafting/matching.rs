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

const CRAFTING_TABLE_GRID_SIZE: usize = 3;
const CRAFTING_TABLE_SLOT_COUNT: usize = CRAFTING_TABLE_GRID_SIZE * CRAFTING_TABLE_GRID_SIZE;
const CRAFTING_TABLE_CENTER: usize = CRAFTING_TABLE_GRID_SIZE / 2;
const CRAFTING_TABLE_LAST_INDEX: usize = CRAFTING_TABLE_GRID_SIZE - 1;
const CRAFTING_TABLE_CENTER_SLOT: usize = CRAFTING_TABLE_SLOT_COUNT / 2;
const DECORATED_POT_INGREDIENT_COUNT: usize = 4;
const MAX_BANNER_PATTERNS: usize = 6;
const MIN_FIREWORK_ROCKET_FUEL_COUNT: usize = 1;
const MAX_FIREWORK_ROCKET_FUEL_COUNT: usize = 3;
const MAX_MAP_SCALE: u8 = 4;

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

fn occupied_stacks(input: &CraftingInput) -> impl Iterator<Item = &ItemStack> {
    input.items.iter().filter(|stack| !stack.is_empty())
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
    let items: Vec<_> = occupied_stacks(input).collect();
    let mut used = vec![false; items.len()];
    match_shapeless_ingredients(&recipe.ingredients, &items, &mut used)
}

fn match_shapeless_ingredients(
    ingredients: &[Ingredient],
    items: &[&ItemStack],
    used: &mut [bool],
) -> bool {
    let Some((ingredient, remaining_ingredients)) = ingredients.split_first() else {
        return true;
    };
    for (item_index, item) in items.iter().enumerate() {
        if used[item_index] || !ingredient.test(item) {
            continue;
        }
        used[item_index] = true;
        if match_shapeless_ingredients(remaining_ingredients, items, used) {
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
    for stack in occupied_stacks(input) {
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
    for stack in occupied_stacks(input) {
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
    input.width == CRAFTING_TABLE_GRID_SIZE
        && input.height == CRAFTING_TABLE_GRID_SIZE
        && input.ingredient_count() == DECORATED_POT_INGREDIENT_COUNT
        && recipe.back.test(input.get(CRAFTING_TABLE_CENTER, 0))
        && recipe.left.test(input.get(0, CRAFTING_TABLE_CENTER))
        && recipe
            .right
            .test(input.get(CRAFTING_TABLE_LAST_INDEX, CRAFTING_TABLE_CENTER))
        && recipe
            .front
            .test(input.get(CRAFTING_TABLE_CENTER, CRAFTING_TABLE_LAST_INDEX))
}

fn imbue(recipe: &ImbueRecipe, input: &CraftingInput) -> bool {
    if input.width != CRAFTING_TABLE_GRID_SIZE
        || input.height != CRAFTING_TABLE_GRID_SIZE
        || input.ingredient_count() != CRAFTING_TABLE_SLOT_COUNT
    {
        return false;
    }
    for y in 0..CRAFTING_TABLE_GRID_SIZE {
        for x in 0..CRAFTING_TABLE_GRID_SIZE {
            let ingredient = if x == CRAFTING_TABLE_CENTER && y == CRAFTING_TABLE_CENTER {
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
    for stack in occupied_stacks(input) {
        if !recipe.banner.test(stack) {
            return false;
        }
        let pattern_count = stack
            .get(BANNER_PATTERNS)
            .map_or(0, |patterns| patterns.layers().len());
        if pattern_count > MAX_BANNER_PATTERNS {
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
    for stack in occupied_stacks(input) {
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
    for stack in occupied_stacks(input) {
        if recipe.shell.test(stack) {
            if has_shell {
                return false;
            }
            has_shell = true;
        } else if recipe.fuel.test(stack) {
            fuel_count += 1;
            if fuel_count > MAX_FIREWORK_ROCKET_FUEL_COUNT {
                return false;
            }
        } else if !recipe.star.test(stack) {
            return false;
        }
    }
    has_shell && fuel_count >= MIN_FIREWORK_ROCKET_FUEL_COUNT
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
    for stack in occupied_stacks(input) {
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
    for stack in occupied_stacks(input) {
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

/// Matches the fixed 3×3 map-scaling layout and its saved-map restrictions.
///
/// The source map's [`super::MapRecipeData`] must be attached to the center
/// input slot so exploration maps and maps at the maximum scale are rejected.
fn map_extending(recipe: &MapExtendingRecipe, input: &CraftingInput) -> bool {
    if input.width != CRAFTING_TABLE_GRID_SIZE
        || input.height != CRAFTING_TABLE_GRID_SIZE
        || input.ingredient_count() != CRAFTING_TABLE_SLOT_COUNT
    {
        return false;
    }
    for y in 0..CRAFTING_TABLE_GRID_SIZE {
        for x in 0..CRAFTING_TABLE_GRID_SIZE {
            let ingredient = if x == CRAFTING_TABLE_CENTER && y == CRAFTING_TABLE_CENTER {
                &recipe.map
            } else {
                &recipe.material
            };
            if !ingredient.test(input.get(x, y)) {
                return false;
            }
        }
    }
    // TODO: Populate this from saved map data once maps are implemented.
    let Some(data) = input.map_data(CRAFTING_TABLE_CENTER_SLOT) else {
        return false;
    };
    !data.exploration_map && data.scale < MAX_MAP_SCALE
}

fn repair_item(_recipe: &RepairItemRecipe, input: &CraftingInput) -> bool {
    if input.ingredient_count() != 2 {
        return false;
    }
    let mut stacks = occupied_stacks(input);
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
    for stack in occupied_stacks(input) {
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

#[cfg(test)]
mod tests {
    use crate::item_stack_template::ItemStackTemplate;
    use crate::recipe::RecipeProperties;
    use crate::{init_vanilla_registry, vanilla_items};

    use super::*;

    fn asymmetric_shaped_recipe(pattern: Vec<Ingredient>) -> ShapedRecipe {
        ShapedRecipe::new(
            RecipeProperties::special(),
            pattern.len(),
            1,
            pattern.into_boxed_slice(),
            ItemStackTemplate::new(&vanilla_items::PURPLE_DYE),
        )
    }

    #[test]
    fn asymmetric_shaped_recipe_matches_both_orientations() {
        init_vanilla_registry();
        let recipe = asymmetric_shaped_recipe(vec![
            Ingredient::Item(&vanilla_items::RED_DYE),
            Ingredient::Item(&vanilla_items::BLUE_DYE),
        ]);
        let forward = CraftingInput::new(
            2,
            1,
            vec![
                ItemStack::new(&vanilla_items::RED_DYE),
                ItemStack::new(&vanilla_items::BLUE_DYE),
            ],
        );
        let mirrored = CraftingInput::new(
            2,
            1,
            vec![
                ItemStack::new(&vanilla_items::BLUE_DYE),
                ItemStack::new(&vanilla_items::RED_DYE),
            ],
        );

        assert!(shaped(&recipe, &forward));
        assert!(shaped(&recipe, &mirrored));
    }

    #[test]
    fn shaped_recipe_requires_empty_pattern_slots() {
        init_vanilla_registry();
        let recipe = asymmetric_shaped_recipe(vec![
            Ingredient::Item(&vanilla_items::RED_DYE),
            Ingredient::Empty,
            Ingredient::Item(&vanilla_items::BLUE_DYE),
        ]);
        let valid = CraftingInput::new(
            3,
            1,
            vec![
                ItemStack::new(&vanilla_items::RED_DYE),
                ItemStack::empty(),
                ItemStack::new(&vanilla_items::BLUE_DYE),
            ],
        );
        let shifted = CraftingInput::new(
            3,
            1,
            vec![
                ItemStack::new(&vanilla_items::RED_DYE),
                ItemStack::new(&vanilla_items::BLUE_DYE),
                ItemStack::empty(),
            ],
        );

        assert!(shaped(&recipe, &valid));
        assert!(!shaped(&recipe, &shifted));
    }

    #[test]
    fn firework_rocket_requires_one_to_three_fuel_items() {
        init_vanilla_registry();
        let recipe = FireworkRocketRecipe {
            properties: RecipeProperties::special(),
            shell: Ingredient::Item(&vanilla_items::PAPER),
            fuel: Ingredient::Item(&vanilla_items::GUNPOWDER),
            star: Ingredient::Item(&vanilla_items::FIREWORK_STAR),
            result: ItemStackTemplate::new(&vanilla_items::FIREWORK_ROCKET),
        };
        let input_with_fuel = |fuel_count| {
            let mut items = vec![ItemStack::new(&vanilla_items::PAPER)];
            items.extend(
                std::iter::repeat_with(|| ItemStack::new(&vanilla_items::GUNPOWDER))
                    .take(fuel_count),
            );
            CraftingInput::new(items.len(), 1, items)
        };

        assert!(!firework_rocket(&recipe, &input_with_fuel(0)));
        assert!(firework_rocket(
            &recipe,
            &input_with_fuel(MIN_FIREWORK_ROCKET_FUEL_COUNT)
        ));
        assert!(firework_rocket(
            &recipe,
            &input_with_fuel(MAX_FIREWORK_ROCKET_FUEL_COUNT)
        ));
        assert!(!firework_rocket(
            &recipe,
            &input_with_fuel(MAX_FIREWORK_ROCKET_FUEL_COUNT + 1)
        ));
    }
}
