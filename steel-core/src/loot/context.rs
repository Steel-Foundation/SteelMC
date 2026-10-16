use super::{LootError, LootTableBehaviorRegistry};
use crate::{block_entity::BlockEntity, entity::Entity, world::World};
use steel_registry::{
    DyeColor, REGISTRY, RegistryExt, equipment::EquipmentSlot, item_stack::ItemStack,
};
use steel_utils::{BlockStateId, Identifier, random::RandomSource};

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

/// Context for loot table evaluation, containing all relevant game state.
///
/// This mirrors vanilla's `LootContext` / `LootParams` system.
pub struct LootContext<'a> {
    /// World access for compiled gameplay operations.
    pub world: Option<&'a World>,
    /// Random number generator.
    pub rng: &'a mut RandomSource,
    pub(super) visited: Vec<Identifier>,
    /// Optional callable registry override, useful for custom compiled tables.
    pub tables: Option<&'a LootTableBehaviorRegistry>,
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
    /// Snapshot and live capability access for entity predicates.
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
    pub block_entity: Option<&'a dyn BlockEntity>,
    /// The entity interacting with a block/entity (e.g., player opening a chest).
    pub interacting_entity: Option<EntityRef<'a>>,
}

/// Weather state for `WeatherCheck` condition.
#[derive(Debug, Clone, Copy, Default)]
pub struct WeatherState {
    /// Whether rain is currently active.
    pub raining: bool,
    /// Whether thunder is currently active.
    pub thundering: bool,
}

/// A reference to an entity for loot context.
/// Snapshot fields avoid holding entity locks during evaluation. Live living-entity
/// access is available for equipment checks.
#[derive(Clone, Copy, Default)]
pub struct EntityRef<'a> {
    /// Live access for compiled operations requiring additional entity capabilities.
    pub entity: Option<&'a dyn Entity>,
    /// Type identifier for the entity.
    pub entity_type: Option<&'a Identifier>,
    /// Entity flags for predicate checking.
    pub flags: EntityRefFlags,
    /// Equipment slots for equipment predicates.
    pub equipment: Option<&'a EntityEquipmentRef<'a>>,
    /// Entity name (for `copy_name` function).
    pub custom_name: Option<&'a str>,
    /// Vanilla `minecraft:components.sheep/color` entity data component.
    pub sheep_color: Option<DyeColor>,
    /// Vanilla `minecraft:type_specific/sheep.sheared`. `None` when the entity is
    /// not a sheep, matching vanilla `SheepPredicate.matches`' non-sheep rejection.
    pub sheep_sheared: Option<bool>,
    /// Vanilla `minecraft:components.chicken/variant` entity data component key.
    pub chicken_variant: Option<&'a Identifier>,
}

/// Entity flags for predicate checking.
#[derive(Debug, Clone, Copy, Default)]
pub struct EntityRefFlags {
    /// Entity is burning.
    pub is_on_fire: bool,
    /// Entity is crouching.
    pub is_sneaking: bool,
    /// Entity is sprinting.
    pub is_sprinting: bool,
    /// Entity is swimming.
    pub is_swimming: bool,
    /// Entity is a baby.
    pub is_baby: bool,
}

/// Equipment references for an entity.
#[derive(Debug, Clone, Copy)]
pub struct EntityEquipmentRef<'a> {
    /// Main hand snapshot.
    pub mainhand: Option<&'a ItemStack>,
    /// Off hand snapshot.
    pub offhand: Option<&'a ItemStack>,
    /// Head equipment snapshot.
    pub head: Option<&'a ItemStack>,
    /// Chest equipment snapshot.
    pub chest: Option<&'a ItemStack>,
    /// Leg equipment snapshot.
    pub legs: Option<&'a ItemStack>,
    /// Feet equipment snapshot.
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

impl<'a> LootContext<'a> {
    /// Create a new loot context with just an RNG.
    pub const fn new(rng: &'a mut RandomSource) -> Self {
        Self {
            world: None,
            rng,
            visited: Vec::new(),
            tables: None,
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

    /// Provides world access without acquiring or retaining a world lock.
    #[must_use]
    pub const fn with_world(mut self, world: &'a World) -> Self {
        self.world = Some(world);
        self
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

    // TODO: Supply the radius from explosion block-drop callers only when the
    // vanilla interaction mode enables decay.
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
    pub const fn with_block_entity(mut self, block_entity: &'a dyn BlockEntity) -> Self {
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

impl LootContext<'_> {
    /// Adds table context to an unsupported-operation error.
    #[must_use]
    pub fn unsupported(&self, operation: &'static str) -> LootError {
        LootError::Unsupported {
            table: self.visited.last().cloned(),
            operation,
        }
    }

    /// Enchantment level over the attacking living entity's applicable equipment slots.
    #[must_use]
    pub fn attacker_enchantment_level(&self, key: &Identifier) -> i32 {
        let Some(entity) = self
            .killer_entity
            .and_then(|entity| entity.entity)
            .and_then(Entity::as_living_entity)
        else {
            return 0;
        };
        let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
            return 0;
        };
        let mut best = 0;
        for slot in EquipmentSlot::ALL {
            if !enchantment.matching_slot(slot) {
                continue;
            }
            entity.with_equipment_slot(slot, &mut |item| {
                best = best.max(item.get_enchantment_level(key));
            });
        }
        best
    }
}
