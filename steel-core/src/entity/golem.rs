//! Golem behavior system.
//!
//! Mirrors Vanilla's golem behavior - Iron Golems protect villages and
//! attack mobs, while Snow Golems throw snowballs and leave a snow trail.

use steel_utils::ErasedType;

use crate::entity::Mob;
use crate::entity_living::LivingEntity;
use crate::entity::IronGolem;
use crate::entity::SnowGolem;
use crate::entity_projectile::Snowball;
use crate::item_stack::ItemStack;
use crate::world::World;
use cgmath::Vector3;
use steel_utils::BlockPos;

// Helper: check if entity is a villager
fn is_villager(entity: &dyn LivingEntity) -> bool {
    // Would check entity type - minecraft:villager
    false
}

// Helper: get distance between two block positions
fn distance_between(pos1: BlockPos, pos2: BlockPos) -> f32 {
    let dx = pos1.x - pos2.x;
    let dy = pos1.y - pos2.y;
    let dz = pos1.z - pos2.z;
    (dx * dx + dy * dy + dz * dz) as f32
}

// Iron Golem behavior
#[derive(Clone, Debug)]
pub struct IronGolemAI {
    /// Whether the golem is angry
    pub angry: bool,
    /// The target the golem is attacking
    pub target: Option<ErasedType>,
    /// How long the golem has been angry
    pub anger_time: i32,
    /// The village this golem is protecting
    pub village: Option<steel_utils::Identifier>,
    /// Cooldown between village patrol ticks
    pub patrol_cooldown: i32,
}

impl IronGolemAI {
    /// Creates a new iron golem AI.
    #[must_use]
    pub fn new() -> Self {
        Self {
            angry: false,
            target: None,
            anger_time: 0,
            village: None,
            patrol_cooldown: 0,
        }
    }

    /// Sets the golem to be angry at a target.
    pub fn set_angry(&mut self, target: ErasedType) {
        self.angry = true;
        self.target = Some(target);
        self.anger_time = 600; // 60 seconds at 20 tps
    }

    /// Clears the anger state.
    pub fn clear_angry(&mut self) {
        self.angry = false;
        self.target = None;
        self.anger_time = 0;
    }

    /// Ticks the golem's anger state.
    pub fn tick_anger(&mut self) {
        if self.angry && self.anger_time > 0 {
            self.anger_time -= 1;
            if self.anger_time == 0 {
                self.clear_angry();
            }
        }
    }

    /// Whether the golem should attack the given entity.
    pub fn should_attack(&self, entity: &dyn LivingEntity) -> bool {
        self.angry && self.target.is_some()
    }

    /// Get the attack damage of the iron golem.
    #[must_use]
    pub fn attack_damage(&self) -> f32 {
        10.0 // Iron golem base damage
    }

    /// Get the health of the iron golem.
    #[must_use]
    pub fn max_health(&self) -> f32 {
        100.0 // Iron golem max health
    }

    /// Village protection logic - checks if golem should target an entity
    pub fn check_village_protection(
        &mut self,
        entity: &dyn LivingEntity,
        villagers: &[&dyn LivingEntity],
        world: &World,
    ) {
        // If entity is attacking a villager, golem should become angry
        for villager in villagers {
            // Check if this entity is in combat with the villager
            // Would check combat events, damage sources, etc.
            // if is_entity_attacking_villager(entity, villager, world) {
            //     self.set_angry(entity.erase_type());
            //     return;
            // }
        }
    }

    /// Village patrol logic - finds closest villager to patrol around.
    pub fn find_patrol_target(&self, villagers: &[&dyn LivingEntity], golem_pos: BlockPos) -> Option<BlockPos> {
        let mut closest: Option<BlockPos> = None;
        let mut closest_dist = f32::MAX;

        for villager in villagers {
            let villager_pos = villager.position();
            let dist = distance_between(golem_pos, villager_pos);
            if dist < closest_dist {
                closest_dist = dist;
                closest = Some(villager_pos);
            }
        }

        closest
    }

    /// Check if there are enough villagers and doors for golem to spawn/be active
    pub fn check_village_requirements(
        &self,
        villagers: &[&dyn LivingEntity],
        doors: &[BlockPos],
        world: &World,
    ) -> bool {
        // Vanilla requirements:
        // - At least 10 villagers
        // - At least 20 "doors" (front doors with air blocks next to them)
        
        let villager_count = villagers.len();
        let door_count = doors.len();
        
        villager_count >= 10 && door_count >= 20
    }
}

/// Snow Golem behavior
#[derive(Clone, Debug)]
pub struct SnowGolemAI {
    /// Whether the golem has its snow effect active
    pub is_snowing: bool,
    /// How long until the snow trail expires
    pub snow_timer: i32,
    /// Whether the golem is currently wet (melting)
    pub is_wet: bool,
    /// Cooldown until next snowball can be thrown
    pub snowball_cooldown: i32,
    /// The snowball entity that was thrown
    pub thrown_snowball: Option<SharedEntity>,
}

impl SnowGolemAI {
    /// Creates a new snow golem AI.
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_snowing: true,
            snow_timer: 600, // 30 seconds
            is_wet: false,
            snowball_cooldown: 0,
            thrown_snowball: None,
        }
    }

    /// Ticks the snow golem's state.
    pub fn tick(&mut self, world: &World) {
        // Snow golems melt in rain or warm biomes
        if self.is_wet {
            self.snow_timer -= 1;
            if self.snow_timer <= 0 {
                // Snow golem disappears
                self.is_snowing = false;
                // Remove the golem from the world
                if let Some(entity) = world.get_entity_by_id(/* golem entity id */) {
                    world.remove_entity(entity);
                }
            }
        }

        // Throw snowball at target if available
        if self.can_throw_snowball() && self.snowball_cooldown <= 0 {
            // Would throw snowball at target
            // self.throw_snowball(world);
            self.snowball_cooldown = 20; // 1 second cooldown
            // Create snowball entity at golem position
            // self.thrown_snowball = Some(snowball_entity);
        }

        // Decrease cooldown
        if self.snowball_cooldown > 0 {
            self.snowball_cooldown -= 1;
        }
    }

    /// Whether the golem is currently producing a snow trail.
    #[must_use]
    pub fn produces_snow_trail(&self) -> bool {
        self.is_snowing && !self.is_wet
    }

    /// Whether the golem's snowball attack is ready.
    pub fn can_throw_snowball(&self) -> bool {
        self.is_snowing && !self.is_wet
    }

    /// Get the snow golem's attack damage.
    #[must_use]
    pub fn attack_damage(&self) -> f32 {
        4.0 // Snow golem snowball damage
    }

    /// Get the snow golem's health.
    #[must_use]
    pub fn max_health(&self) -> f32 {
        4.0 // Snow golem max health
    }
}

/// Handles an iron golem becoming angry.
///
/// Mirrors Vanilla's `IronGolem.setAngry()` - makes the golem attack
/// the specified entity and starts the anger timer.
pub fn set_iron_golem_angry(
    golem: &IronGolem,
    target: &dyn LivingEntity,
    world: &World,
) {
    let mut ai = golem.ai_mut();
    ai.set_angry(target.erase_type());
    // Would also broadcast the anger to clients
}

/// Handles an iron golem calming down.
pub fn clear_iron_golem_angry(golem: &IronGolem) {
    let mut ai = golem.ai_mut();
    ai.clear_angry();
}

/// Handles a snow golem melting.
pub fn tick_snow_golem(&mut golem: &SnowGolem, world: &World) {
    let mut ai = golem.ai_mut();
    ai.tick(world);
    // Would check if snow golem should disappear
    if !ai.is_snowing {
        // Golem disappears
        golem.die(steel_utils::DamageSource::generic());
    }
}

/// Gets the iron golem's AI state.
pub fn iron_golem_ai(golem: &IronGolem) -> &IronGolemAI {
    golem.ai()
}

/// Gets the snow golem's AI state.
pub fn snow_golem_ai(golem: &SnowGolem) -> &SnowGolemAI {
    golem.ai()
}