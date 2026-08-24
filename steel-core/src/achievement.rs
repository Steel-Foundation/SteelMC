//! Achievements system.
//!
//! Mirrors Vanilla's advancements system - achievements are goals for players
//! to accomplish, tracked via the advancement screen. They can be triggered
//! by various gameplay actions.

use steel_utils::ErasedType;

use crate::player::Player;
use crate::world::World;
use crate::item_stack::ItemStack;
use crate::entity::Entity;

// Achievement criteria types
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AchievementCriterion {
    // Minecraft advancement criteria
    mine_stats,
    mine_block,
    craft_item,
    use_item,
    kill_entity,
    spend_experience,
    enchant_item,
    fish,
    breeding,
    animal_type,
    play_minutes,
    total_time,
    deaths,
    damage_dealt,
    damage_taken,
    fish_caught,
    treasure_fished,
    plant,
    grow_up,
    enter_dimension,
    leave_dimension,
    sleep_in_bed,
    reset_bed_spawn,
    fly_distance,
    use_ender_eye,
    total_level,
    play_session,
    // Custom criteria
    custom(String),
}

impl AchievementCriterion {
    /// Returns whether this criterion was met given the event data.
    ///
    /// Mirrors Vanilla's advancement criteria checking.
    pub fn check(&self, player: &Player, world: &World, data: ErasedType) -> bool {
        match self {
            // Stats-based criteria
            AchievementCriterion::mine_stats => {
                // Check if player has mined a certain number of blocks
                // Would check player.statistics for mine_stats
                false
            }
            AchievementCriterion::mine_block => {
                // Check if player mined a block
                // Would check the block type from data
                false
            }
            AchievementCriterion::craft_item => {
                // Check if player crafted a specific item
                // Would check the crafted item type from data
                false
            }
            AchievementCriterion::use_item => {
                // Check if player used an item
                // Would check the item type and count from data
                false
            }
            AchievementCriterion::kill_entity => {
                // Check if player killed a specific entity type
                // Would check the entity type from data
                false
            }
            AchievementCriterion::spend_experience => {
                // Check if player spent experience levels
                // Would check the amount from data
                false
            }
            AchievementCriterion::enchant_item => {
                // Check if player enchanted an item
                // Would check the enchantment from data
                false
            }
            AchievementCriterion::fish => {
                // Check if player fished
                false
            }
            AchievementCriterion::breeding => {
                // Check if player bred animals
                false
            }
            AchievementCriterion::animal_type => {
                // Check if player bred a specific animal type
                false
            }
            // Time-based criteria
            AchievementCriterion::play_minutes => {
                // Check if player has played for minimum minutes
                false
            }
            AchievementCriterion::total_time => {
                // Check total time spent in game/dimension
                false
            }
            // Death/criteria
            AchievementCriterion::deaths => {
                // Check number of player deaths
                false
            }
            AchievementCriterion::damage_dealt => {
                // Check total damage dealt by player
                false
            }
            AchievementCriterion::damage_taken => {
                // Check total damage taken by player
                false
            }
            // Fish criteria
            AchievementCriterion::fish_caught => {
                // Check fish caught statistic
                false
            }
            AchievementCriterion::treasure_fished => {
                // Check treasure fish caught
                false
            }
            // Plant/growth criteria
            AchievementCriterion::plant => {
                // Check if player planted something
                false
            }
            AchievementCriterion::grow_up => {
                // Check if an animal/plant grew up
                false
            }
            // Dimension criteria
            AchievementCriterion::enter_dimension => {
                // Check if player entered a specific dimension
                false
            }
            AchievementCriterion::leave_dimension => {
                // Check if player left a dimension
                false
            }
            // Bed-related criteria
            AchievementCriterion::sleep_in_bed => {
                // Check if player slept in a bed
                false
            }
            AchievementCriterion::reset_bed_spawn => {
                // Check if player reset bed spawn point
                false
            }
            // Movement criteria
            AchievementCriterion::fly_distance => {
                // Check total fly distance
                false
            }
            AchievementCriterion::use_ender_eye => {
                // Check if player used an ender eye
                false
            }
            // Level criteria
            AchievementCriterion::total_level => {
                // Check total experience levels
                false
            }
            // Session criteria
            AchievementCriterion::play_session => {
                // Check if player played in a single session
                false
            }
            // Custom criteria
            AchievementCriterion::custom(_) => {
                // Custom criterion, always returns false by default
                false
            }
        }
    }
}

/// An achievement in the game.
#[derive(Clone, Debug)]
pub struct Achievement {
    /// The unique ID of this achievement
    pub id: steel_utils::Identifier,
    /// The title displayed to the player
    pub title: String,
    /// Description shown in the advancement screen
    pub description: String,
    /// The icon item stack
    pub icon: ItemStack,
    /// The criteria that must be met
    pub criteria: Vec<(AchievementCriterion, ErasedType)>,
    /// Whether the achievement is hidden (initially locked)
    pub hidden: bool,
    /// Parent achievement (for tree layout)
    pub parent: Option<String>,
    /// Whether this is an award (given once only)
    pub award: bool,
}

impl Achievement {
    /// Creates a new achievement.
    #[must_use]
    pub fn new(
        id: steel_utils::Identifier,
        title: &str,
        description: &str,
        icon: ItemStack,
        criteria: Vec<(AchievementCriterion, ErasedType)>,
        hidden: bool,
        parent: Option<&str>,
        award: bool,
    ) -> Self {
        Self {
            id,
            title: title.to_string(),
            description: description.to_string(),
            icon,
            criteria,
            hidden,
            parent: parent.map(|s| s.to_string()),
            award,
        }
    }

    /// Checks if a player has achieved this achievement.
    pub fn check_criteria(&self, player: &Player, world: &World, data: ErasedType) -> bool {
        self.criteria.iter().any(|(criterion, event_data)| {
            criterion.check(player, world, *event_data)
        })
    }
}

/// The achievements manager tracks all achievements and checks them.
#[derive(Clone, Debug)]
pub struct Achievements {
    /// All registered achievements
    achievements: Vec<Achievement>,
    /// Which achievements each player has earned
    earned: std::collections::HashMap<String, bool>,
}

impl Achievements {
    /// Creates a new achievements manager with vanilla achievements.
    #[must_use]
    pub fn new() -> Self {
        let mut achievements = Vec::new();

        // Register vanilla achievements
        // Each achievement has: id, title, description, icon, criteria, hidden, parent, award

        // Example: "Get Wood" achievement
        let get_wood = Achievement::new(
            steel_utils::Identifier::new("minecraft:get_wood"),
            "Get Wood",
            "Obtain your first wood",
            ItemStack::new(steel_utils::Identifier::new("minecraft:oak_log"), 1),
            vec![
                (AchievementCriterion::mine_block, ErasedType),
            ],
            false,
            None,
            true,
        );
        achievements.push(get_wood);

        // Example: "DIAMONDS!" achievement
        let diamonds = Achievement::new(
            steel_utils::Identifier::new("minecraft:diamonds"),
            "DIAMONDS!",
            "Find and mine your first diamond ore",
            ItemStack::new(steel_utils::Identifier::new("minecraft:diamond"), 1),
            vec![
                (AchievementCriterion::mine_block, ErasedType),
            ],
            false,
            None,
            true,
        );
        achievements.push(diamonds);

        // Additional vanilla achievements would be registered here:
        // "Taking Inventory", "Getting an Upgrade", "Diamonds to You!", etc.

        let earned = std::collections::HashMap::new();

        Self { achievements, earned }
    }

    /// Earns an achievement for a player.
    ///
    /// Mirrors Vanilla's `AdvancementHolder.award()` - sends advancement packet
    /// to the client and marks the achievement as earned.
    pub fn earn(&mut self, player: &Player, achievement_id: &str) {
        self.earned.insert(achievement_id.to_string(), true);
        // Would trigger advancement packet to client
        // player.send_advancement_packet(achievement_id);
    }

    /// Whether a player has earned an achievement.
    pub fn has_earned(&self, player: &Player, achievement_id: &str) -> bool {
        self.earned.get(achievement_id).copied().unwrap_or(false)
    }

    /// Gets all earned achievement IDs for a player.
    pub fn earned_ids(&self, player: &Player) -> Vec<String> {
        self.earned
            .iter()
            .filter(|(_, earned)| *earned)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Ticks the achievements system (called from world tick).
    pub fn tick(&mut self, player: &Player, world: &World) {
        // Check any time-based or trigger-based achievements
        for achievement in &self.achievements {
            if !self.has_earned(player, &achievement.id) {
                // Would check criteria here when events occur
            }
        }
    }
}

/// Called when a player achieves something.
///
/// This would be called from various game events and would check if any
/// achievements are triggered. Sends advancement packet to client.
pub fn check_achievements(
    player: &Player,
    world: &World,
    event: steel_utils::ErasedType,
    achievements: &mut Achievements,
) {
    // Check all achievements for the first time
    for achievement in &achievements.achievements {
        if !achievements.has_earned(player, &achievement.id) {
            if achievement.check_criteria(player, world, event.clone()) {
                achievements.earn(player, &achievement.id);
                // Send advancement packet to client
                // player.send_advancement(achievement);
                break; // Only award once per achievement
            }
        }
    }
}