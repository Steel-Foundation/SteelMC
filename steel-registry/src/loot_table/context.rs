use super::{BlockStateId, Identifier, ItemStack, RngExt};

/// Entity target for loot context lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootContextEntity {
    /// The entity being looted (killed mob, block entity owner).
    This,
    /// The entity that killed the target.
    Killer,
    /// The direct attacker (e.g., arrow, not the player who shot it).
    DirectKiller,
    /// The player who dealt the final damage.
    KillerPlayer,
    /// The entity interacting with a block/entity.
    Interacting,
}

/// The type of loot table, determining when/how it's used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootType {
    Block,
    Entity,
    Chest,
    Fishing,
    Gift,
    Archaeology,
    Vault,
    Shearing,
    Equipment,
    Selector,
    EntityInteract,
    BlockInteract,
    Barter,
}

/// A number provider that can be constant or random.
#[derive(Debug, Clone)]
pub enum NumberProvider {
    Constant(f32),
    Uniform {
        min: f32,
        max: f32,
    },
    Binomial {
        n: i32,
        p: f32,
    },
    /// Get value from entity scoreboard score.
    Score {
        target: ScoreboardTarget,
        score: &'static str,
        scale: f32,
    },
    /// Get value from command storage.
    Storage {
        storage: Identifier,
        path: &'static str,
    },
    /// Get enchantment level from context tool.
    EnchantmentLevel {
        enchantment: Identifier,
    },
}

/// Target for scoreboard number provider.
#[derive(Debug, Clone, Copy)]
pub enum ScoreboardTarget {
    /// The entity being looted.
    This,
    /// The entity that killed the target.
    Killer,
    /// The direct killer (e.g., arrow vs player).
    DirectKiller,
    /// The player who dealt the last damage.
    KillerPlayer,
    /// A fixed entity name.
    Fixed(&'static str),
}

impl NumberProvider {
    /// Get a value from this provider using the given RNG.
    pub fn get<R: rand::Rng>(&self, rng: &mut R, ctx: Option<&LootContextRef<'_>>) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform { min, max } => rng.random_range(*min..=*max),
            Self::Binomial { n, p } => {
                let mut count = 0;
                for _ in 0..*n {
                    if rng.random::<f32>() < *p {
                        count += 1;
                    }
                }
                count as f32
            }
            Self::Score { .. } => {
                // TODO: Implement when scoreboard system is available
                let _ = ctx;
                0.0
            }
            Self::Storage { .. } => {
                // TODO: Implement when command storage system is available
                let _ = ctx;
                0.0
            }
            Self::EnchantmentLevel { enchantment } => ctx
                .and_then(|c| c.tool)
                .map_or(0.0, |t| t.get_enchantment_level(enchantment) as f32),
        }
    }

    /// Get a value without context (for backwards compatibility).
    pub fn get_simple(&self, rng: &mut impl rand::Rng) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform { min, max } => rng.random_range(*min..=*max),
            Self::Binomial { n, p } => {
                let mut count = 0;
                for _ in 0..*n {
                    if rng.random::<f32>() < *p {
                        count += 1;
                    }
                }
                count as f32
            }
            // Context-dependent providers return 0 when no context available
            Self::Score { .. } | Self::Storage { .. } | Self::EnchantmentLevel { .. } => 0.0,
        }
    }

    /// Get the value as an integer.
    pub fn get_int(&self, rng: &mut impl rand::Rng) -> i32 {
        self.get_simple(rng).floor() as i32
    }

    /// Get the value as an integer with context.
    pub fn get_int_with_ctx<R: rand::Rng>(
        &self,
        rng: &mut R,
        ctx: Option<&LootContextRef<'_>>,
    ) -> i32 {
        self.get(rng, ctx).floor() as i32
    }
}

/// A range for number comparisons (used in `ValueCheck`, `TimeCheck`, `EntityScores`).
#[derive(Debug, Clone)]
pub struct NumberProviderRange {
    pub min: Option<NumberProvider>,
    pub max: Option<NumberProvider>,
}

impl NumberProviderRange {
    /// Check if a value is within this range.
    pub fn test(&self, value: f32, rng: &mut impl rand::Rng) -> bool {
        if let Some(min) = &self.min
            && value < min.get_simple(rng)
        {
            return false;
        }
        if let Some(max) = &self.max
            && value > max.get_simple(rng)
        {
            return false;
        }
        true
    }

    /// Create an exact match range.
    #[must_use]
    pub const fn exact(value: f32) -> Self {
        Self {
            min: Some(NumberProvider::Constant(value)),
            max: Some(NumberProvider::Constant(value)),
        }
    }

    /// Create an at-least range.
    #[must_use]
    pub const fn at_least(min: f32) -> Self {
        Self {
            min: Some(NumberProvider::Constant(min)),
            max: None,
        }
    }

    /// Create an at-most range.
    #[must_use]
    pub const fn at_most(max: f32) -> Self {
        Self {
            min: None,
            max: Some(NumberProvider::Constant(max)),
        }
    }

    /// Create a between range.
    #[must_use]
    pub const fn between(min: f32, max: f32) -> Self {
        Self {
            min: Some(NumberProvider::Constant(min)),
            max: Some(NumberProvider::Constant(max)),
        }
    }
}

/// Reference to loot context for number provider evaluation.
/// This is a simplified view to avoid circular references.
pub struct LootContextRef<'a> {
    pub tool: Option<&'a ItemStack>,
    // Add more fields as needed for Score/Storage providers
}

/// Context for loot table evaluation, containing all relevant game state.
///
/// This mirrors vanilla's `LootContext` / `LootParams` system.
pub struct LootContext<'a, R: rand::Rng> {
    /// Random number generator.
    pub rng: &'a mut R,
    /// Luck value (e.g., from Luck of the Sea enchantment).
    pub luck: f32,
    /// The block state being broken (for block loot tables).
    pub block_state: Option<BlockStateId>,
    /// The tool used to break the block.
    pub tool: Option<&'a ItemStack>,
    /// Explosion radius if caused by an explosion.
    pub explosion_radius: Option<f32>,
    /// Whether the entity was killed by a player.
    pub killed_by_player: bool,

    /// World position where the loot is generated (block position or entity death location).
    pub origin: Option<(f64, f64, f64)>,
    /// Current game time in ticks (for `TimeCheck` condition).
    pub game_time: Option<i64>,
    /// Current weather state.
    pub weather: Option<WeatherState>,
    /// The entity being looted (the killed mob, block entity owner, etc.).
    /// This is a type-erased pointer; actual entity data depends on game implementation.
    pub this_entity: Option<EntityRef<'a>>,
    /// The entity that killed `this_entity` (for mob loot tables).
    pub killer_entity: Option<EntityRef<'a>>,
    /// The direct attacker entity (e.g., arrow, not the player who shot it).
    pub direct_killer_entity: Option<EntityRef<'a>>,
    /// The player who dealt the final damage (may be different from killer).
    pub last_damage_player: Option<EntityRef<'a>>,
    /// Damage source information for entity deaths.
    pub damage_source: Option<DamageSourceInfo<'a>>,
    /// Block entity reference for container/block loot.
    pub block_entity: Option<BlockEntityRef<'a>>,
    /// The entity interacting with a block/entity (e.g., player opening a chest).
    pub interacting_entity: Option<EntityRef<'a>>,
}

/// Weather state for `WeatherCheck` condition.
#[derive(Debug, Clone, Copy, Default)]
pub struct WeatherState {
    pub raining: bool,
    pub thundering: bool,
}

/// A reference to an entity for loot context.
/// This is intentionally opaque - the actual entity type depends on game implementation.
#[derive(Debug, Clone, Copy)]
pub struct EntityRef<'a> {
    /// Type identifier for the entity.
    pub entity_type: Option<&'a Identifier>,
    /// Entity flags for predicate checking.
    pub flags: EntityRefFlags,
    /// Equipment slots for equipment predicates.
    pub equipment: Option<&'a EntityEquipmentRef<'a>>,
    /// Entity name (for `copy_name` function).
    pub custom_name: Option<&'a str>,
}

/// Entity flags for predicate checking.
#[derive(Debug, Clone, Copy, Default)]
pub struct EntityRefFlags {
    pub is_on_fire: bool,
    pub is_sneaking: bool,
    pub is_sprinting: bool,
    pub is_swimming: bool,
    pub is_baby: bool,
}

/// Equipment references for an entity.
#[derive(Debug, Clone, Copy)]
pub struct EntityEquipmentRef<'a> {
    pub mainhand: Option<&'a ItemStack>,
    pub offhand: Option<&'a ItemStack>,
    pub head: Option<&'a ItemStack>,
    pub chest: Option<&'a ItemStack>,
    pub legs: Option<&'a ItemStack>,
    pub feet: Option<&'a ItemStack>,
}

/// Damage source information for loot context.
#[derive(Debug, Clone, Copy)]
pub struct DamageSourceInfo<'a> {
    /// The damage type identifier.
    pub damage_type: Option<&'a Identifier>,
    /// Tags associated with this damage source.
    pub tags: &'a [Identifier],
    /// Whether this is direct damage (not from a projectile).
    pub is_direct: bool,
}

/// A reference to a block entity for loot context.
#[derive(Debug, Clone, Copy)]
pub struct BlockEntityRef<'a> {
    /// The block entity type identifier.
    pub block_entity_type: Option<&'a Identifier>,
    /// Custom name of the block entity (for `copy_name`).
    pub custom_name: Option<&'a str>,
    /// Inventory contents (for dynamic/slots entries).
    pub inventory: Option<&'a [ItemStack]>,
}

impl<'a, R: rand::Rng> LootContext<'a, R> {
    /// Create a new loot context with just an RNG.
    pub const fn new(rng: &'a mut R) -> Self {
        Self {
            rng,
            luck: 0.0,
            block_state: None,
            tool: None,
            explosion_radius: None,
            killed_by_player: false,
            origin: None,
            game_time: None,
            weather: None,
            this_entity: None,
            killer_entity: None,
            direct_killer_entity: None,
            last_damage_player: None,
            damage_source: None,
            block_entity: None,
            interacting_entity: None,
        }
    }

    /// Set the luck value.
    #[must_use]
    pub const fn with_luck(mut self, luck: f32) -> Self {
        self.luck = luck;
        self
    }

    /// Set the block state.
    #[must_use]
    pub const fn with_block_state(mut self, state: BlockStateId) -> Self {
        self.block_state = Some(state);
        self
    }

    /// Set the tool used.
    #[must_use]
    pub const fn with_tool(mut self, tool: &'a ItemStack) -> Self {
        self.tool = Some(tool);
        self
    }

    /// Set the explosion radius.
    #[must_use]
    pub const fn with_explosion(mut self, radius: f32) -> Self {
        self.explosion_radius = Some(radius);
        self
    }

    /// Set whether killed by player.
    #[must_use]
    pub const fn with_killed_by_player(mut self, killed: bool) -> Self {
        self.killed_by_player = killed;
        self
    }

    /// Set the world origin position.
    #[must_use]
    pub const fn with_origin(mut self, x: f64, y: f64, z: f64) -> Self {
        self.origin = Some((x, y, z));
        self
    }

    /// Set the game time.
    #[must_use]
    pub const fn with_game_time(mut self, time: i64) -> Self {
        self.game_time = Some(time);
        self
    }

    /// Set the weather state.
    #[must_use]
    pub const fn with_weather(mut self, weather: WeatherState) -> Self {
        self.weather = Some(weather);
        self
    }

    /// Set the entity being looted.
    #[must_use]
    pub const fn with_this_entity(mut self, entity: EntityRef<'a>) -> Self {
        self.this_entity = Some(entity);
        self
    }

    /// Set the killer entity.
    #[must_use]
    pub const fn with_killer_entity(mut self, entity: EntityRef<'a>) -> Self {
        self.killer_entity = Some(entity);
        self
    }

    /// Set the direct killer entity (e.g., projectile).
    #[must_use]
    pub const fn with_direct_killer_entity(mut self, entity: EntityRef<'a>) -> Self {
        self.direct_killer_entity = Some(entity);
        self
    }

    /// Set the player who dealt the final damage.
    #[must_use]
    pub const fn with_last_damage_player(mut self, entity: EntityRef<'a>) -> Self {
        self.last_damage_player = Some(entity);
        self
    }

    /// Set the damage source information.
    #[must_use]
    pub const fn with_damage_source(mut self, damage_source: DamageSourceInfo<'a>) -> Self {
        self.damage_source = Some(damage_source);
        self
    }

    /// Set the block entity reference.
    #[must_use]
    pub const fn with_block_entity(mut self, block_entity: BlockEntityRef<'a>) -> Self {
        self.block_entity = Some(block_entity);
        self
    }

    /// Set the interacting entity (e.g., player opening a chest).
    #[must_use]
    pub const fn with_interacting_entity(mut self, entity: EntityRef<'a>) -> Self {
        self.interacting_entity = Some(entity);
        self
    }

    /// Get the level of an enchantment on the tool by identifier.
    #[must_use]
    pub fn get_enchantment_level_by_id(&self, enchantment: &Identifier) -> i32 {
        self.tool
            .map_or(0, |t| t.get_enchantment_level(enchantment))
    }

    /// Get an entity reference by target.
    #[must_use]
    pub const fn get_entity(&self, target: LootContextEntity) -> Option<EntityRef<'a>> {
        match target {
            LootContextEntity::This => self.this_entity,
            LootContextEntity::Killer => self.killer_entity,
            LootContextEntity::DirectKiller => self.direct_killer_entity,
            LootContextEntity::KillerPlayer => self.last_damage_player,
            LootContextEntity::Interacting => self.interacting_entity,
        }
    }
}
