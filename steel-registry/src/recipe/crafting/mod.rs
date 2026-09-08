//! Passive data and immutable input snapshots for crafting recipes.

mod matching;

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::data_components::vanilla_components::FireworkExplosionShape;
use crate::item_stack::ItemStack;
use crate::item_stack_template::ItemStackTemplate;

use super::{Ingredient, RecipeData, RecipeInput, RecipeMatches, RecipeProperties};

/// A shaped recipe pattern.
#[derive(Debug)]
pub struct ShapedRecipe {
    pub properties: RecipeProperties,
    pub width: usize,
    pub height: usize,
    pub pattern: Box<[Ingredient]>,
    pub result: ItemStackTemplate,
    /// Whether the pattern is horizontally symmetric.
    pub symmetrical: bool,
}

impl ShapedRecipe {
    #[must_use]
    pub fn new(
        mut properties: RecipeProperties,
        width: usize,
        height: usize,
        pattern: Box<[Ingredient]>,
        result: ItemStackTemplate,
    ) -> Self {
        let symmetrical = Self::compute_symmetrical(width, &pattern);
        if properties.placement.is_none() {
            properties.placement = Some(super::PlacementInfo::from_optional_ingredients(&pattern));
        }
        Self {
            properties,
            width,
            height,
            pattern,
            result,
            symmetrical,
        }
    }

    #[must_use]
    pub fn with_placement(mut self, placement: super::PlacementInfo) -> Self {
        self.properties.placement = Some(placement);
        self
    }

    fn compute_symmetrical(width: usize, pattern: &[Ingredient]) -> bool {
        if width == 0 {
            return true;
        }
        let height = pattern.len() / width;
        for y in 0..height {
            for x in 0..width / 2 {
                if !pattern[y * width + x].eq_ingredient(&pattern[y * width + (width - 1 - x)]) {
                    return false;
                }
            }
        }
        true
    }

    #[must_use]
    pub const fn fits_in(&self, width: usize, height: usize) -> bool {
        self.width <= width && self.height <= height
    }
}

/// A shapeless collection of ingredients.
#[derive(Debug)]
pub struct ShapelessRecipe {
    pub properties: RecipeProperties,
    pub ingredients: Box<[Ingredient]>,
    pub result: ItemStackTemplate,
}

impl ShapelessRecipe {
    #[must_use]
    pub fn new(
        mut properties: RecipeProperties,
        ingredients: Box<[Ingredient]>,
        result: ItemStackTemplate,
    ) -> Self {
        if properties.placement.is_none() {
            properties.placement = Some(super::PlacementInfo::from_ingredients(&ingredients));
        }
        Self {
            properties,
            ingredients,
            result,
        }
    }

    #[must_use]
    pub fn with_placement(mut self, placement: super::PlacementInfo) -> Self {
        self.properties.placement = Some(placement);
        self
    }

    #[must_use]
    pub const fn fits_in(&self, width: usize, height: usize) -> bool {
        self.ingredients.len() <= width * height
    }
}

#[derive(Debug)]
pub struct TransmuteRecipe {
    pub properties: RecipeProperties,
    pub input: Ingredient,
    pub material: Ingredient,
    pub min_material_count: usize,
    pub max_material_count: usize,
    pub result: ItemStackTemplate,
    pub add_material_count_to_result: bool,
}

#[derive(Debug)]
pub struct DyeRecipe {
    pub properties: RecipeProperties,
    pub target: Ingredient,
    pub dye: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct DecoratedPotRecipe {
    pub properties: RecipeProperties,
    pub back: Ingredient,
    pub left: Ingredient,
    pub right: Ingredient,
    pub front: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct ImbueRecipe {
    pub properties: RecipeProperties,
    pub source: Ingredient,
    pub material: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct BannerDuplicateRecipe {
    pub properties: RecipeProperties,
    pub banner: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct BookCloningRecipe {
    pub properties: RecipeProperties,
    pub source: Ingredient,
    pub material: Ingredient,
    pub min_generation: i32,
    pub max_generation: i32,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct FireworkRocketRecipe {
    pub properties: RecipeProperties,
    pub shell: Ingredient,
    pub fuel: Ingredient,
    pub star: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct FireworkStarRecipe {
    pub properties: RecipeProperties,
    pub shapes: Box<[(FireworkExplosionShape, Ingredient)]>,
    pub trail: Ingredient,
    pub twinkle: Ingredient,
    pub fuel: Ingredient,
    pub dye: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct FireworkStarFadeRecipe {
    pub properties: RecipeProperties,
    pub target: Ingredient,
    pub dye: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct MapExtendingRecipe {
    pub properties: RecipeProperties,
    pub map: Ingredient,
    pub material: Ingredient,
    pub result: ItemStackTemplate,
}

#[derive(Debug)]
pub struct RepairItemRecipe {
    pub properties: RecipeProperties,
}

#[derive(Debug)]
pub struct ShieldDecorationRecipe {
    pub properties: RecipeProperties,
    pub banner: Ingredient,
    pub target: Ingredient,
    pub result: ItemStackTemplate,
}

/// Every vanilla serializer whose operational type is `minecraft:crafting`.
#[derive(Debug)]
pub enum CraftingRecipe {
    Shaped(ShapedRecipe),
    Shapeless(ShapelessRecipe),
    Transmute(TransmuteRecipe),
    Dye(DyeRecipe),
    DecoratedPot(DecoratedPotRecipe),
    Imbue(ImbueRecipe),
    BannerDuplicate(BannerDuplicateRecipe),
    BookCloning(BookCloningRecipe),
    FireworkRocket(FireworkRocketRecipe),
    FireworkStar(FireworkStarRecipe),
    FireworkStarFade(FireworkStarFadeRecipe),
    MapExtending(MapExtendingRecipe),
    RepairItem(RepairItemRecipe),
    ShieldDecoration(ShieldDecorationRecipe),
}

impl CraftingRecipe {
    #[must_use]
    pub const fn properties(&self) -> &RecipeProperties {
        match self {
            Self::Shaped(recipe) => &recipe.properties,
            Self::Shapeless(recipe) => &recipe.properties,
            Self::Transmute(recipe) => &recipe.properties,
            Self::Dye(recipe) => &recipe.properties,
            Self::DecoratedPot(recipe) => &recipe.properties,
            Self::Imbue(recipe) => &recipe.properties,
            Self::BannerDuplicate(recipe) => &recipe.properties,
            Self::BookCloning(recipe) => &recipe.properties,
            Self::FireworkRocket(recipe) => &recipe.properties,
            Self::FireworkStar(recipe) => &recipe.properties,
            Self::FireworkStarFade(recipe) => &recipe.properties,
            Self::MapExtending(recipe) => &recipe.properties,
            Self::RepairItem(recipe) => &recipe.properties,
            Self::ShieldDecoration(recipe) => &recipe.properties,
        }
    }

    #[must_use]
    pub const fn result(&self) -> Option<&ItemStackTemplate> {
        match self {
            Self::Shaped(recipe) => Some(&recipe.result),
            Self::Shapeless(recipe) => Some(&recipe.result),
            Self::Transmute(recipe) => Some(&recipe.result),
            Self::Dye(recipe) => Some(&recipe.result),
            Self::DecoratedPot(recipe) => Some(&recipe.result),
            Self::Imbue(recipe) => Some(&recipe.result),
            Self::BannerDuplicate(recipe) => Some(&recipe.result),
            Self::BookCloning(recipe) => Some(&recipe.result),
            Self::FireworkRocket(recipe) => Some(&recipe.result),
            Self::FireworkStar(recipe) => Some(&recipe.result),
            Self::FireworkStarFade(recipe) => Some(&recipe.result),
            Self::MapExtending(recipe) => Some(&recipe.result),
            Self::RepairItem(_) => None,
            Self::ShieldDecoration(recipe) => Some(&recipe.result),
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies the unified vanilla crafting data enum.
unsafe impl DowncastType for CraftingRecipe {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_data/crafting");
}

impl RecipeData for CraftingRecipe {
    fn properties(&self) -> Option<&RecipeProperties> {
        Some(self.properties())
    }
}

/// Saved-map facts needed by the vanilla map-extending matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapRecipeData {
    pub exploration_map: bool,
    pub scale: u8,
}

/// Positioned, immutable crafting-grid snapshot.
#[derive(Debug, Clone)]
pub struct CraftingInput {
    pub width: usize,
    pub height: usize,
    pub items: Vec<ItemStack>,
    ingredient_count: usize,
    map_data: Vec<Option<MapRecipeData>>,
}

impl CraftingInput {
    pub const EMPTY: Self = Self {
        width: 0,
        height: 0,
        items: Vec::new(),
        ingredient_count: 0,
        map_data: Vec::new(),
    };

    #[must_use]
    pub fn new(width: usize, height: usize, items: Vec<ItemStack>) -> Self {
        debug_assert_eq!(items.len(), width * height);
        let ingredient_count = items.iter().filter(|stack| !stack.is_empty()).count();
        let map_data = vec![None; items.len()];
        Self {
            width,
            height,
            items,
            ingredient_count,
            map_data,
        }
    }

    #[must_use]
    pub fn with_map_data(mut self, slot: usize, data: MapRecipeData) -> Self {
        if let Some(map_data) = self.map_data.get_mut(slot) {
            *map_data = Some(data);
        }
        self
    }

    #[must_use]
    pub fn map_data(&self, slot: usize) -> Option<MapRecipeData> {
        self.map_data.get(slot).copied().flatten()
    }

    #[must_use]
    pub fn positioned(
        width: usize,
        height: usize,
        items: Vec<ItemStack>,
    ) -> PositionedCraftingInput {
        if width == 0 || height == 0 {
            return PositionedCraftingInput::EMPTY;
        }

        let mut left = width;
        let mut right = 0;
        let mut top = height;
        let mut bottom = 0;
        for y in 0..height {
            for x in 0..width {
                if !items[y * width + x].is_empty() {
                    left = left.min(x);
                    right = right.max(x);
                    top = top.min(y);
                    bottom = bottom.max(y);
                }
            }
        }
        if left > right || top > bottom {
            return PositionedCraftingInput::EMPTY;
        }

        let new_width = right - left + 1;
        let new_height = bottom - top + 1;
        if new_width == width && new_height == height {
            return PositionedCraftingInput {
                input: Self::new(width, height, items),
                left,
                top,
            };
        }

        let mut positioned = Vec::with_capacity(new_width * new_height);
        for y in 0..new_height {
            for x in 0..new_width {
                positioned.push(items[(x + left) + (y + top) * width].clone());
            }
        }
        PositionedCraftingInput {
            input: Self::new(new_width, new_height, positioned),
            left,
            top,
        }
    }

    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> &ItemStack {
        &self.items[y * self.width + x]
    }

    #[must_use]
    pub const fn ingredient_count(&self) -> usize {
        self.ingredient_count
    }
}

// SAFETY: This Steel-owned key uniquely identifies a crafting matching snapshot.
unsafe impl DowncastType for CraftingInput {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:recipe_input/crafting");
}

impl RecipeInput for CraftingInput {
    fn is_empty(&self) -> bool {
        self.ingredient_count == 0
    }
}

impl RecipeMatches<CraftingInput> for CraftingRecipe {
    fn matches(&self, input: &CraftingInput) -> bool {
        matching::matches(self, input)
    }
}

/// Positioned input plus its offset in the original crafting grid.
#[derive(Debug, Clone)]
pub struct PositionedCraftingInput {
    pub input: CraftingInput,
    pub left: usize,
    pub top: usize,
}

impl PositionedCraftingInput {
    pub const EMPTY: Self = Self {
        input: CraftingInput::EMPTY,
        left: 0,
        top: 0,
    };

    #[must_use]
    pub const fn to_grid_slot(&self, x: usize, y: usize, grid_width: usize) -> usize {
        (x + self.left) + (y + self.top) * grid_width
    }
}
