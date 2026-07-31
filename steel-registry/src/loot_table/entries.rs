use super::{
    ConditionalLootFunction, Identifier, ItemStack, LootCondition, LootContext, LootType,
    NumberProvider, REGISTRY, RegistryExt, RngExt, TaggedRegistryExt,
};

/// A loot table entry that can generate items.
#[derive(Debug, Clone)]
pub enum LootEntry {
    /// Drop a specific item.
    Item {
        name: Identifier,
        weight: i32,
        quality: i32,
        conditions: &'static [LootCondition],
        functions: &'static [ConditionalLootFunction],
    },
    /// Reference another loot table by name.
    LootTableRef {
        name: Identifier,
        weight: i32,
        quality: i32,
        conditions: &'static [LootCondition],
        functions: &'static [ConditionalLootFunction],
    },
    /// Inline loot table (embedded pools directly in entry).
    InlineLootTable {
        pools: &'static [LootPool],
        weight: i32,
        quality: i32,
        conditions: &'static [LootCondition],
        functions: &'static [ConditionalLootFunction],
    },
    /// Drop items from a tag.
    Tag {
        name: Identifier,
        expand: bool,
        weight: i32,
        quality: i32,
        conditions: &'static [LootCondition],
        functions: &'static [ConditionalLootFunction],
    },
    /// Try children in order, use first that matches.
    Alternatives {
        children: &'static [LootEntry],
        conditions: &'static [LootCondition],
    },
    /// Use all children.
    Group {
        children: &'static [LootEntry],
        conditions: &'static [LootCondition],
    },
    /// Use children in sequence until one fails.
    Sequence {
        children: &'static [LootEntry],
        conditions: &'static [LootCondition],
    },
    /// Empty entry (no drop).
    Empty {
        weight: i32,
        conditions: &'static [LootCondition],
    },
    /// Dynamic content (e.g., block entity contents).
    Dynamic {
        name: Identifier,
        conditions: &'static [LootCondition],
    },
    /// Select items from specific block entity slots.
    Slots {
        /// Slots to select from (can be single slot or range).
        slots: SlotRange,
        conditions: &'static [LootCondition],
        functions: &'static [ConditionalLootFunction],
    },
}

/// A range of slots for the Slots entry type.
#[derive(Debug, Clone, Copy)]
pub enum SlotRange {
    /// A single specific slot index.
    Single(i32),
    /// A range of slots (inclusive).
    Range { min: i32, max: i32 },
    /// All contents slots.
    Contents,
    /// Specific named slots (for entities).
    Named(&'static [&'static str]),
}

impl LootEntry {
    /// Get the weight of this entry for random selection.
    #[must_use]
    pub const fn weight(&self) -> i32 {
        match self {
            Self::Item { weight, .. } => *weight,
            Self::LootTableRef { weight, .. } => *weight,
            Self::InlineLootTable { weight, .. } => *weight,
            Self::Tag { weight, .. } => *weight,
            Self::Empty { weight, .. } => *weight,
            // Composite entries don't have weight
            Self::Alternatives { .. }
            | Self::Group { .. }
            | Self::Sequence { .. }
            | Self::Dynamic { .. }
            | Self::Slots { .. } => 1,
        }
    }

    /// Get the quality modifier for luck-based weight adjustment.
    #[must_use]
    pub const fn quality(&self) -> i32 {
        match self {
            Self::Item { quality, .. } => *quality,
            Self::LootTableRef { quality, .. } => *quality,
            Self::InlineLootTable { quality, .. } => *quality,
            Self::Tag { quality, .. } => *quality,
            Self::Empty { .. }
            | Self::Alternatives { .. }
            | Self::Group { .. }
            | Self::Sequence { .. }
            | Self::Dynamic { .. }
            | Self::Slots { .. } => 0,
        }
    }

    /// Get the effective weight adjusted for luck.
    /// Formula: max(floor(weight + quality * luck), 0)
    #[must_use]
    pub fn effective_weight(&self, luck: f32) -> i32 {
        let base = self.weight() as f32;
        let quality = self.quality() as f32;
        (base + quality * luck).floor().max(0.0) as i32
    }

    /// Get the conditions for this entry.
    #[must_use]
    pub const fn conditions(&self) -> &'static [LootCondition] {
        match self {
            Self::Item { conditions, .. } => conditions,
            Self::LootTableRef { conditions, .. } => conditions,
            Self::InlineLootTable { conditions, .. } => conditions,
            Self::Tag { conditions, .. } => conditions,
            Self::Alternatives { conditions, .. } => conditions,
            Self::Group { conditions, .. } => conditions,
            Self::Sequence { conditions, .. } => conditions,
            Self::Empty { conditions, .. } => conditions,
            Self::Dynamic { conditions, .. } => conditions,
            Self::Slots { conditions, .. } => conditions,
        }
    }

    /// Get the functions for this entry.
    #[must_use]
    pub const fn functions(&self) -> &'static [ConditionalLootFunction] {
        match self {
            Self::Item { functions, .. } => functions,
            Self::LootTableRef { functions, .. } => functions,
            Self::InlineLootTable { functions, .. } => functions,
            Self::Tag { functions, .. } => functions,
            Self::Slots { functions, .. } => functions,
            Self::Empty { .. }
            | Self::Alternatives { .. }
            | Self::Group { .. }
            | Self::Sequence { .. }
            | Self::Dynamic { .. } => &[],
        }
    }
}

/// A pool of loot entries with roll counts.
#[derive(Debug, Clone)]
pub struct LootPool {
    pub rolls: NumberProvider,
    pub bonus_rolls: f32,
    pub entries: &'static [LootEntry],
    pub conditions: &'static [LootCondition],
    pub functions: &'static [ConditionalLootFunction],
}

/// A complete loot table definition.
#[derive(Debug)]
pub struct LootTable {
    pub key: Identifier,
    pub loot_type: LootType,
    pub pools: &'static [LootPool],
    pub functions: &'static [ConditionalLootFunction],
    pub random_sequence: Option<Identifier>,
}

impl LootTable {
    /// Generate random items from this loot table.
    ///
    /// # Arguments
    /// * `ctx` - The loot context containing RNG, luck, block state, tool, etc.
    ///
    /// This follows vanilla's approach:
    /// 1. For each pool, check conditions
    /// 2. Roll `rolls + floor(bonus_rolls * luck)` times
    /// 3. Each roll does weighted random selection among valid entries
    /// 4. Apply entry-level functions to each item
    /// 5. Apply pool-level functions to all items from that pool
    /// 6. Apply table-level functions to all items from the table
    pub fn get_random_items<R: rand::Rng>(&self, ctx: &mut LootContext<'_, R>) -> Vec<ItemStack> {
        let mut result = Vec::new();
        for pool in self.pools {
            pool.add_random_items(ctx, &mut result);
        }

        // Apply table-level functions to all items
        if !self.functions.is_empty() {
            for item in &mut result {
                for cond_func in self.functions {
                    if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                        cond_func.function.apply(item, ctx);
                    }
                }
            }
            // Remove items with zero count after applying functions
            result.retain(|item| item.count > 0);
        }

        result
    }
}

impl LootPool {
    /// Add random items from this pool to the result.
    fn add_random_items<R: rand::Rng>(
        &self,
        ctx: &mut LootContext<'_, R>,
        result: &mut Vec<ItemStack>,
    ) {
        // Check pool conditions
        for condition in self.conditions {
            if !condition.test(ctx) {
                return;
            }
        }

        // Track where items from this pool start
        let start_index = result.len();

        // Calculate number of rolls
        let roll_count = self.rolls.get_int(ctx.rng) + (self.bonus_rolls * ctx.luck).floor() as i32;

        for _ in 0..roll_count {
            self.add_random_item(ctx, result);
        }

        // Apply pool-level functions to all items generated by this pool
        if !self.functions.is_empty() {
            for item in result.iter_mut().skip(start_index) {
                for cond_func in self.functions {
                    if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                        cond_func.function.apply(item, ctx);
                    }
                }
            }
            // Remove items with zero count after applying functions
            result.retain(|item| item.count > 0);
        }
    }

    /// Select and add a single random item from this pool.
    fn add_random_item<R: rand::Rng>(
        &self,
        ctx: &mut LootContext<'_, R>,
        result: &mut Vec<ItemStack>,
    ) {
        // Collect valid entries with their effective weights
        let mut valid_entries: Vec<(&LootEntry, i32)> = Vec::new();
        let mut total_weight = 0;

        for entry in self.entries {
            // Check entry conditions
            let passes_conditions = entry.conditions().iter().all(|c| c.test(ctx));

            if !passes_conditions {
                continue;
            }

            let weight = entry.effective_weight(ctx.luck);
            if weight > 0 {
                valid_entries.push((entry, weight));
                total_weight += weight;
            }
        }

        if total_weight == 0 || valid_entries.is_empty() {
            return;
        }

        // Weighted random selection
        let selected = if valid_entries.len() == 1 {
            valid_entries[0].0
        } else {
            let mut index = ctx.rng.random_range(0..total_weight);
            let mut selected_entry = valid_entries[0].0;
            for (entry, weight) in &valid_entries {
                index -= weight;
                if index < 0 {
                    selected_entry = entry;
                    break;
                }
            }
            selected_entry
        };

        // Generate item(s) from the selected entry
        selected.create_items(ctx, result);
    }
}

impl LootEntry {
    /// Create items from this entry and add them to the result.
    fn create_items<R: rand::Rng>(
        &self,
        ctx: &mut LootContext<'_, R>,
        result: &mut Vec<ItemStack>,
    ) {
        match self {
            LootEntry::Item {
                name, functions, ..
            } => {
                if let Some(item_ref) = REGISTRY.items.by_key(name) {
                    let mut item = ItemStack::new(item_ref);

                    // Apply functions
                    for cond_func in *functions {
                        if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                            cond_func.function.apply(&mut item, ctx);
                        }
                    }

                    if item.count > 0 {
                        result.push(item);
                    }
                }
            }
            LootEntry::LootTableRef {
                name, functions, ..
            } => {
                // Recursively get items from referenced loot table
                if let Some(table) = REGISTRY.loot_tables.by_key(name) {
                    let mut items = table.get_random_items(ctx);
                    // Apply functions to all items from the referenced table
                    for item in &mut items {
                        for cond_func in *functions {
                            if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                                cond_func.function.apply(item, ctx);
                            }
                        }
                    }
                    result.extend(items.into_iter().filter(|i| i.count > 0));
                }
            }
            LootEntry::InlineLootTable {
                pools, functions, ..
            } => {
                // Process inline loot table pools directly
                let mut items = Vec::new();
                for pool in *pools {
                    pool.add_random_items(ctx, &mut items);
                }
                // Apply functions to all items from the inline table
                for item in &mut items {
                    for cond_func in *functions {
                        if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                            cond_func.function.apply(item, ctx);
                        }
                    }
                }
                result.extend(items.into_iter().filter(|i| i.count > 0));
            }
            LootEntry::Tag {
                name,
                expand,
                functions,
                ..
            } => {
                // Get all items in the tag
                if let Some(items) = REGISTRY.items.get_tag(name) {
                    if *expand {
                        // Pick one random item from the tag (weighted equally)
                        if !items.is_empty() {
                            let index = ctx.rng.random_range(0..items.len());
                            let mut item = ItemStack::new(items[index]);
                            for cond_func in *functions {
                                if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                                    cond_func.function.apply(&mut item, ctx);
                                }
                            }
                            if item.count > 0 {
                                result.push(item);
                            }
                        }
                    } else {
                        // Drop all items from the tag
                        for item_ref in items {
                            let mut item = ItemStack::new(item_ref);
                            for cond_func in *functions {
                                if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                                    cond_func.function.apply(&mut item, ctx);
                                }
                            }
                            if item.count > 0 {
                                result.push(item);
                            }
                        }
                    }
                }
            }
            LootEntry::Alternatives { children, .. } => {
                // Try children in order, use first that passes conditions and produces items
                for child in *children {
                    // Check child's conditions first
                    let passes_conditions = child.conditions().iter().all(|c| c.test(ctx));
                    if !passes_conditions {
                        continue; // Try next alternative
                    }

                    let before_len = result.len();
                    child.create_items(ctx, result);
                    if result.len() > before_len {
                        break; // First successful child that produced items, stop
                    }
                }
            }
            LootEntry::Group { children, .. } => {
                // Use all children that pass their conditions
                for child in *children {
                    let passes_conditions = child.conditions().iter().all(|c| c.test(ctx));
                    if passes_conditions {
                        child.create_items(ctx, result);
                    }
                }
            }
            LootEntry::Sequence { children, .. } => {
                // Use children in sequence until one fails its conditions
                // Note: Unlike Alternatives, Sequence stops when conditions FAIL,
                // not when items are produced. A child can produce nothing but still "succeed".
                for child in *children {
                    let passes_conditions = child.conditions().iter().all(|c| c.test(ctx));
                    if !passes_conditions {
                        break; // Condition failed, stop sequence
                    }
                    child.create_items(ctx, result);
                }
            }
            LootEntry::Empty { .. } => {
                // Empty entry produces nothing
            }
            LootEntry::Dynamic { name, .. } => {
                // Dynamic entries are used for block entity contents (like shulker boxes)
                // The name identifies what content to retrieve:
                // - "contents" = block entity inventory contents
                // - Other names may exist for specific use cases
                //
                // TODO: Implement when block entity system supports inventory retrieval
                // This requires:
                // 1. Block entity reference in LootContext
                // 2. Method to get inventory contents from block entity
                // 3. Adding those items to the result
                let _ = name;
            }
            LootEntry::Slots {
                slots, functions, ..
            } => {
                // Slots entries select items from specific block entity slots
                // TODO: Implement when block entity system supports slot access
                // This requires:
                // 1. Block entity reference in LootContext
                // 2. Method to get items from specific slots
                // 3. Apply functions to each retrieved item
                let _ = slots;
                let _ = functions;
            }
        }
    }
}
