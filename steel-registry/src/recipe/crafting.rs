//! Crafting recipe types (shaped and shapeless).

use steel_utils::Identifier;

use crate::{item_stack::ItemStack, items::ItemRef};

use super::ingredient::Ingredient;

/// Category for crafting recipes (used by recipe book).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftingCategory {
    Building,
    Redstone,
    Equipment,
    Misc,
}

impl CraftingCategory {
    /// Parses a category from a JSON string.
    #[must_use]
    pub fn parse_json(s: &str) -> Self {
        match s {
            "building" => Self::Building,
            "redstone" => Self::Redstone,
            "equipment" => Self::Equipment,
            _ => Self::Misc,
        }
    }
}

/// The result of a crafting recipe.
#[derive(Debug, Clone)]
pub struct RecipeResult {
    pub item: ItemRef,
    pub count: i32,
}

impl RecipeResult {
    /// Creates an `ItemStack` from this result.
    #[must_use]
    pub fn to_item_stack(&self) -> ItemStack {
        ItemStack::with_count(self.item, self.count)
    }
}

/// A shaped crafting recipe with a specific pattern.
#[derive(Debug)]
pub struct ShapedRecipe {
    pub id: Identifier,
    pub category: CraftingCategory,
    pub width: usize,
    pub height: usize,
    /// Pattern ingredients in row-major order (width * height).
    pub pattern: &'static [Ingredient],
    pub result: RecipeResult,
    pub show_notification: bool,
    /// Pre-computed: whether the pattern is horizontally symmetric.
    pub symmetrical: bool,
}

impl ShapedRecipe {
    /// Creates a new shaped recipe, pre-computing symmetry.
    #[must_use]
    pub fn new(
        id: Identifier,
        category: CraftingCategory,
        width: usize,
        height: usize,
        pattern: &'static [Ingredient],
        result: RecipeResult,
        show_notification: bool,
    ) -> Self {
        let symmetrical = Self::compute_symmetrical(width, pattern);
        Self {
            id,
            category,
            width,
            height,
            pattern,
            result,
            show_notification,
            symmetrical,
        }
    }

    /// Computes whether the pattern is horizontally symmetric.
    fn compute_symmetrical(width: usize, pattern: &[Ingredient]) -> bool {
        if width == 0 {
            return true;
        }
        let height = pattern.len() / width;
        for y in 0..height {
            for x in 0..width / 2 {
                let left = &pattern[y * width + x];
                let right = &pattern[y * width + (width - 1 - x)];
                if !left.eq_ingredient(right) {
                    return false;
                }
            }
        }
        true
    }

    /// Returns true if this recipe fits in a 2x2 grid.
    #[must_use]
    pub fn fits_in_2x2(&self) -> bool {
        self.width <= 2 && self.height <= 2
    }

    /// Tests if the crafting input matches this recipe.
    #[must_use]
    pub fn matches(&self, input: &CraftingInput) -> bool {
        // Early exit: ingredient count must match
        if input.ingredient_count != self.pattern.iter().filter(|i| !i.is_empty()).count() {
            return false;
        }

        // Dimensions must match
        if input.width != self.width || input.height != self.height {
            return false;
        }

        // Try normal orientation
        if self.matches_at(input, false) {
            return true;
        }

        // Only try mirrored if not symmetric
        if !self.symmetrical && self.matches_at(input, true) {
            return true;
        }

        false
    }

    /// Tests if the crafting input matches this recipe with optional mirroring.
    fn matches_at(&self, input: &CraftingInput, mirrored: bool) -> bool {
        for y in 0..self.height {
            for x in 0..self.width {
                let pattern_x = if mirrored { self.width - 1 - x } else { x };
                let ingredient = &self.pattern[y * self.width + pattern_x];
                let input_item = input.get(x, y);

                if !ingredient.test(input_item) {
                    return false;
                }
            }
        }
        true
    }

    /// Assembles the result item stack.
    #[must_use]
    pub fn assemble(&self) -> ItemStack {
        self.result.to_item_stack()
    }

    /// Gets the remaining items after crafting (e.g., empty buckets).
    #[must_use]
    pub fn get_remaining_items(&self, input: &CraftingInput) -> Vec<ItemStack> {
        input
            .items
            .iter()
            .map(|stack| {
                if stack.is_empty() {
                    ItemStack::empty()
                } else {
                    stack.item.get_crafting_remainder()
                }
            })
            .collect()
    }
}

/// A shapeless crafting recipe where ingredient order doesn't matter.
#[derive(Debug)]
pub struct ShapelessRecipe {
    pub id: Identifier,
    pub category: CraftingCategory,
    pub ingredients: &'static [Ingredient],
    pub result: RecipeResult,
}

impl ShapelessRecipe {
    /// Returns true if this recipe fits in a 2x2 grid.
    #[must_use]
    pub fn fits_in_2x2(&self) -> bool {
        self.ingredients.len() <= 4
    }

    /// Tests if the crafting input matches this recipe.
    #[must_use]
    pub fn matches(&self, input: &CraftingInput) -> bool {
        // Must have same number of items as ingredients
        if input.ingredient_count != self.ingredients.len() {
            return false;
        }

        // Fast path for single ingredient
        if self.ingredients.len() == 1 {
            return self.ingredients[0].test(input.items.iter().find(|s| !s.is_empty()).unwrap());
        }

        // Try to match each ingredient to an input item
        let non_empty: Vec<&ItemStack> = input.items.iter().filter(|s| !s.is_empty()).collect();
        let mut used = vec![false; non_empty.len()];

        for ingredient in self.ingredients {
            let mut found = false;
            for (i, item) in non_empty.iter().enumerate() {
                if !used[i] && ingredient.test(item) {
                    used[i] = true;
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }

        true
    }

    /// Assembles the result item stack.
    #[must_use]
    pub fn assemble(&self) -> ItemStack {
        self.result.to_item_stack()
    }

    /// Gets the remaining items after crafting (e.g., empty buckets).
    #[must_use]
    pub fn get_remaining_items(&self, input: &CraftingInput) -> Vec<ItemStack> {
        input
            .items
            .iter()
            .map(|stack| {
                if stack.is_empty() {
                    ItemStack::empty()
                } else {
                    stack.item.get_crafting_remainder()
                }
            })
            .collect()
    }
}

/// Unified crafting recipe enum (replaces trait-based approach).
#[derive(Debug)]
pub enum CraftingRecipe {
    Shaped(&'static ShapedRecipe),
    Shapeless(&'static ShapelessRecipe),
}

impl CraftingRecipe {
    /// Returns the recipe identifier.
    #[must_use]
    pub fn id(&self) -> &Identifier {
        match self {
            Self::Shaped(r) => &r.id,
            Self::Shapeless(r) => &r.id,
        }
    }

    /// Returns the recipe category.
    #[must_use]
    pub fn category(&self) -> CraftingCategory {
        match self {
            Self::Shaped(r) => r.category,
            Self::Shapeless(r) => r.category,
        }
    }

    /// Returns the result of this recipe.
    #[must_use]
    pub fn result(&self) -> &RecipeResult {
        match self {
            Self::Shaped(r) => &r.result,
            Self::Shapeless(r) => &r.result,
        }
    }

    /// Tests if the crafting input matches this recipe.
    /// The input should already be positioned/trimmed.
    #[must_use]
    pub fn matches(&self, input: &CraftingInput) -> bool {
        match self {
            Self::Shaped(r) => r.matches(input),
            Self::Shapeless(r) => r.matches(input),
        }
    }

    /// Assembles the result item stack.
    #[must_use]
    pub fn assemble(&self) -> ItemStack {
        match self {
            Self::Shaped(r) => r.assemble(),
            Self::Shapeless(r) => r.assemble(),
        }
    }

    /// Gets the remaining items after crafting (e.g., empty buckets).
    #[must_use]
    pub fn get_remaining_items(&self, input: &CraftingInput) -> Vec<ItemStack> {
        match self {
            Self::Shaped(r) => r.get_remaining_items(input),
            Self::Shapeless(r) => r.get_remaining_items(input),
        }
    }

    /// Returns true if this recipe fits in a 2x2 grid.
    #[must_use]
    pub fn fits_in_2x2(&self) -> bool {
        match self {
            Self::Shaped(r) => r.fits_in_2x2(),
            Self::Shapeless(r) => r.fits_in_2x2(),
        }
    }
}

/// Represents the current state of a crafting grid.
///
/// This should be a **positioned** (trimmed) input - containing only the
/// bounding box of non-empty items. Use `CraftingInput::positioned()` to
/// create one from raw grid slots.
#[derive(Debug, Clone)]
pub struct CraftingInput {
    pub width: usize,
    pub height: usize,
    /// Items in row-major order (width * height).
    pub items: Vec<ItemStack>,
    /// Pre-computed count of non-empty items.
    ingredient_count: usize,
}

impl CraftingInput {
    /// An empty crafting input.
    pub const EMPTY: CraftingInput = CraftingInput {
        width: 0,
        height: 0,
        items: Vec::new(),
        ingredient_count: 0,
    };

    /// Creates a new crafting input, pre-computing ingredient count.
    #[must_use]
    pub fn new(width: usize, height: usize, items: Vec<ItemStack>) -> Self {
        debug_assert_eq!(items.len(), width * height);
        let ingredient_count = items.iter().filter(|s| !s.is_empty()).count();
        Self {
            width,
            height,
            items,
            ingredient_count,
        }
    }

    /// Creates a positioned (trimmed) crafting input from raw grid slots.
    ///
    /// This is the main entry point matching Java's `CraftingInput.ofPositioned()`.
    /// Returns the trimmed input along with the offset from the original grid.
    #[must_use]
    pub fn positioned(
        width: usize,
        height: usize,
        items: Vec<ItemStack>,
    ) -> PositionedCraftingInput {
        if width == 0 || height == 0 {
            return PositionedCraftingInput::EMPTY;
        }

        // Find bounding box
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

        // Empty grid
        if left > right || top > bottom {
            return PositionedCraftingInput::EMPTY;
        }

        let new_width = right - left + 1;
        let new_height = bottom - top + 1;

        // If bounds match original, use items directly
        if new_width == width && new_height == height {
            return PositionedCraftingInput {
                input: CraftingInput::new(width, height, items),
                left,
                top,
            };
        }

        // Create trimmed input
        let mut new_items = Vec::with_capacity(new_width * new_height);
        for y in 0..new_height {
            for x in 0..new_width {
                let index = (x + left) + (y + top) * width;
                new_items.push(items[index].clone());
            }
        }

        PositionedCraftingInput {
            input: CraftingInput::new(new_width, new_height, new_items),
            left,
            top,
        }
    }

    /// Gets the item at the specified position.
    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> &ItemStack {
        &self.items[y * self.width + x]
    }

    /// Returns the number of non-empty items (pre-computed).
    #[must_use]
    pub fn ingredient_count(&self) -> usize {
        self.ingredient_count
    }

    /// Returns true if the input is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ingredient_count == 0
    }
}

/// A crafting input with position information.
///
/// This represents a trimmed crafting grid (containing only the bounding box
/// of non-empty items) along with the offset from the original grid origin.
/// This is used when consuming ingredients to correctly map recipe slots back
/// to the original crafting grid slots.
#[derive(Debug, Clone)]
pub struct PositionedCraftingInput {
    /// The trimmed crafting input.
    pub input: CraftingInput,
    /// The X offset from the original grid origin.
    pub left: usize,
    /// The Y offset from the original grid origin.
    pub top: usize,
}

impl PositionedCraftingInput {
    /// An empty positioned crafting input.
    pub const EMPTY: PositionedCraftingInput = PositionedCraftingInput {
        input: CraftingInput::EMPTY,
        left: 0,
        top: 0,
    };

    /// Converts a position in the trimmed input back to the original grid slot index.
    ///
    /// # Arguments
    /// * `x` - X position in the trimmed input (0 to input.width-1)
    /// * `y` - Y position in the trimmed input (0 to input.height-1)
    /// * `grid_width` - Width of the original crafting grid
    ///
    /// # Returns
    /// The slot index in the original crafting grid.
    #[must_use]
    pub fn to_grid_slot(&self, x: usize, y: usize, grid_width: usize) -> usize {
        (x + self.left) + (y + self.top) * grid_width
    }
}
