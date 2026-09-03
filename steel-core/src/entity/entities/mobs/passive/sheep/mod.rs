//! Vanilla Sheep entity with wool color, shearing, and breeding parity.
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::biome::BiomeRef;
use steel_registry::data_components::vanilla_components::DYE;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::recipe::CraftingInput;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_biome_tags;
use steel_registry::vanilla_entity_data::SheepEntityData;
use steel_registry::vanilla_game_events;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_loot_tables;
use steel_registry::{DyeColor, REGISTRY, TaggedRegistryExt, sound_events, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, EatBlockGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal,
    RandomLookAroundGoal, TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::living_entity::shearing_loot_items_with_rng;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, SpawnGroupData,
};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

const SHEEP_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.5625, 0.0)];
const SHEEP_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    0.45,
    0.65,
    0.65625,
    EntityAttachments::new(&SHEEP_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);

/// Low 4 bits of the synced wool byte hold the wool color id.
const COLOR_ID_MASK: i8 = 0x0F;
/// Bit 5 of the synced wool byte marks the sheep as sheared.
const SHEARED_BIT: i8 = 0x10;
/// Vanilla `Sheep.ate` ages babies up by this many seconds (`ageUp(60)`).
const ATE_AGE_UP_SECONDS: i32 = 60;

/// Vanilla `SheepColorSpawnRules` weighted tables, mirroring the warm/cold/temperate
/// configurations with their exact vanilla weights.
const TEMPERATE_SPAWN_COLORS: &[(DyeColor, i32)] = &[
    (DyeColor::Black, 5),
    (DyeColor::Gray, 5),
    (DyeColor::LightGray, 5),
    (DyeColor::Brown, 3),
    (DyeColor::White, 499),
    (DyeColor::Pink, 1),
];
const WARM_SPAWN_COLORS: &[(DyeColor, i32)] = &[
    (DyeColor::Gray, 5),
    (DyeColor::LightGray, 5),
    (DyeColor::White, 5),
    (DyeColor::Black, 3),
    (DyeColor::Brown, 499),
    (DyeColor::Pink, 1),
];
const COLD_SPAWN_COLORS: &[(DyeColor, i32)] = &[
    (DyeColor::LightGray, 5),
    (DyeColor::Gray, 5),
    (DyeColor::White, 5),
    (DyeColor::Brown, 3),
    (DyeColor::Black, 499),
    (DyeColor::Pink, 1),
];

/// Vanilla sheep entity.
#[entity_behavior(class = "Sheep")]
pub struct SheepEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<SheepEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SheepEntity`.
unsafe impl DowncastType for SheepEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/sheep");
}

impl SheepEntity {
    /// Creates a new sheep entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates a sheep entity from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mob_base = MobBase::new();
        let ageable_base = AgeableMobBase::new();
        let animal_base = AnimalBase::new();
        AnimalBase::initialize_pathfinding_malus(&mob_base);
        let mut entity_data = SheepEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            // Keep vanilla Sheep goal priorities and speeds in the same order.
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(1.25));
            goal_selector.add_goal(2, BreedGoal::new(1.0));
            goal_selector.add_goal(
                3,
                TemptGoal::new(
                    1.1,
                    |item_stack| {
                        REGISTRY
                            .items
                            .is_in_tag(item_stack.item(), &ItemTag::SHEEP_FOOD)
                    },
                    false,
                ),
            );
            goal_selector.add_goal(4, FollowParentGoal::new(1.1));
            goal_selector.add_goal(5, EatBlockGoal::new());
            goal_selector.add_goal(6, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(7, LookAtPlayerGoal::new(6.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            entity_data: SyncMutex::new(entity_data),
        }
    }

    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }

        let display = self.living_base.mob_effect_display_state();

        {
            let mut entity_data = self.entity_data.lock();
            let living = entity_data.living_entity_mut();
            living.effect_particles.set(display.particles);
            living.effect_ambience.set(display.ambient);
        }

        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }

    /// Returns whether the stack is vanilla sheep food.
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::SHEEP_FOOD)
    }

    /// Returns the current wool color (vanilla `Sheep.getColor`).
    #[must_use]
    pub fn color(&self) -> DyeColor {
        DyeColor::by_id(i32::from(
            self.entity_data.lock().wool.get() & COLOR_ID_MASK,
        ))
    }

    /// Sets the wool color, preserving the sheared bit (vanilla `Sheep.setColor`).
    pub fn set_color(&self, color: DyeColor) {
        let mut entity_data = self.entity_data.lock();
        let current = *entity_data.wool.get();
        entity_data
            .wool
            .set((current & !COLOR_ID_MASK) | ((color.id() & 15) as i8));
    }

    /// Returns whether this sheep has been sheared (vanilla `Sheep.isSheared`).
    #[must_use]
    pub fn is_sheared(&self) -> bool {
        (*self.entity_data.lock().wool.get() & SHEARED_BIT) != 0
    }

    /// Sets the sheared state, preserving the color bits (vanilla `Sheep.setSheared`).
    pub fn set_sheared(&self, sheared: bool) {
        let mut entity_data = self.entity_data.lock();
        let current = *entity_data.wool.get();
        let next = if sheared {
            current | SHEARED_BIT
        } else {
            current & !SHEARED_BIT
        };
        entity_data.wool.set(next);
    }

    /// Returns vanilla `Sheep.readyForShearing`.
    #[must_use]
    pub fn ready_for_shearing(&self) -> bool {
        !self.is_sheared() && !AgeableMob::is_baby(self)
    }

    /// Runs vanilla `Sheep.shear`: plays the shear sound, drops the shearing loot
    /// one item at a time with the vanilla jitter, and marks the sheep as sheared.
    pub fn shear(&self, world: &World, tool: &ItemStack) {
        world.play_sound_at(
            &sound_events::ENTITY_SHEEP_SHEAR,
            SoundSource::Players,
            self.position(),
            1.0,
            1.0,
            None,
        );

        let mut rng = rand::rng();
        for drop in
            shearing_loot_items_with_rng(self, &vanilla_loot_tables::SHEARING_SHEEP, tool, &mut rng)
        {
            self.spawn_shearing_drop(&drop);
        }

        self.set_sheared(true);
    }

    /// Drops one stack from shearing, spawning a count-1 item entity per count
    /// unit with the vanilla jitter (vanilla `Sheep.shear`'s spawn lambda).
    fn spawn_shearing_drop(&self, drop: &ItemStack) {
        for _ in 0..drop.count() {
            let Some(item_entity) = self.spawn_at_location(drop.copy_with_count(1), 1.0) else {
                continue;
            };
            let jitter = DVec3::new(
                (rand::random::<f64>() - rand::random::<f64>()) * 0.1,
                rand::random::<f64>() * 0.05,
                (rand::random::<f64>() - rand::random::<f64>()) * 0.1,
            );
            item_entity.set_velocity(item_entity.velocity() + jitter);
        }
    }

    /// Returns the mixed offspring color for two parents, mirroring
    /// `DyeColor.getMixedColor`: a 2x1 crafting lookup over all dye items of both
    /// colors, falling back to a random parent when no mix recipe exists.
    #[must_use]
    pub fn get_mixed_color(color1: DyeColor, color2: DyeColor) -> DyeColor {
        Self::find_color_mix_in_recipes(color1, color2).unwrap_or_else(|| {
            if rand::random::<bool>() {
                color1
            } else {
                color2
            }
        })
    }

    fn find_color_mix_in_recipes(color1: DyeColor, color2: DyeColor) -> Option<DyeColor> {
        let dye1: Vec<_> = REGISTRY
            .items
            .iter()
            .filter_map(|(_, item)| (item.components.get(DYE) == Some(color1)).then_some(item))
            .collect();
        if dye1.is_empty() {
            return None;
        }
        let dye2: Vec<_> = REGISTRY
            .items
            .iter()
            .filter_map(|(_, item)| (item.components.get(DYE) == Some(color2)).then_some(item))
            .collect();
        if dye2.is_empty() {
            return None;
        }

        for dye1_item in &dye1 {
            for dye2_item in &dye2 {
                let input = CraftingInput::new(
                    2,
                    1,
                    vec![ItemStack::new(dye1_item), ItemStack::new(dye2_item)],
                );
                let Some(recipe) = REGISTRY.recipes.find_crafting_recipe_2x2(&input) else {
                    continue;
                };
                if let Some(color) = recipe.assemble().get(DYE).copied() {
                    return Some(color);
                }
            }
        }

        None
    }

    /// Returns vanilla `SheepColorSpawnRules.getSheepColor` for the biome.
    #[must_use]
    pub fn random_sheep_color(biome: BiomeRef, random: &mut impl Random) -> DyeColor {
        if biome.has_tag(&vanilla_biome_tags::BiomeTag::SPAWNS_WARM_VARIANT_FARM_ANIMALS) {
            Self::pick_spawn_color(WARM_SPAWN_COLORS, random)
        } else if biome.has_tag(&vanilla_biome_tags::BiomeTag::SPAWNS_COLD_VARIANT_FARM_ANIMALS) {
            Self::pick_spawn_color(COLD_SPAWN_COLORS, random)
        } else {
            Self::pick_spawn_color(TEMPERATE_SPAWN_COLORS, random)
        }
    }

    /// Picks a color from a vanilla weighted table using `WeightedList.get` semantics.
    fn pick_spawn_color(table: &[(DyeColor, i32)], random: &mut impl Random) -> DyeColor {
        let total = table.iter().map(|(_, weight)| weight).sum::<i32>();
        let mut roll = random.next_i32_bounded(total);
        let mut last = DyeColor::White;
        for (color, weight) in table {
            roll -= weight;
            last = *color;
            if roll < 0 {
                return last;
            }
        }
        last
    }
}

impl Entity for SheepEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if AgeableMob::is_baby(self) {
            SHEEP_BABY_DIMENSIONS.scale(scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(&sound_events::ENTITY_SHEEP_STEP, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        nbt.insert("Sheared", i8::from(self.is_sheared()));
        // Vanilla stores `Color` through `DyeColor.LEGACY_ID_CODEC` (a byte).
        nbt.insert("Color", self.color().id() as i8);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);
        self.set_sheared(nbt.byte("Sheared").is_some_and(|value| value != 0));
        self.set_color(DyeColor::by_id(i32::from(nbt.byte("Color").unwrap_or(0))));
    }
}

impl LivingEntity for SheepEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data
            .lock()
            .living_entity_mut()
            .health
            .set(clamped);
    }

    fn sound_volume(&self) -> f32 {
        0.4
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_SHEEP_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_SHEEP_DEATH)
    }

    fn sheep_loot_state(&self) -> Option<(DyeColor, bool)> {
        Some((self.color(), self.is_sheared()))
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = Mob::mob_ai_step(self);

        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        result
    }
}

impl AgeableMob for SheepEntity {
    fn ageable_base(&self) -> &AgeableMobBase {
        &self.ageable_base
    }

    fn is_age_locked(&self) -> bool {
        *self.entity_data.lock().ageable_mob().age_locked.get()
    }

    fn set_age_locked(&self, age_locked: bool) {
        self.entity_data
            .lock()
            .ageable_mob_mut()
            .age_locked
            .set(age_locked);
    }

    fn set_synced_baby(&self, baby: bool) {
        self.entity_data.lock().ageable_mob_mut().baby.set(baby);
    }

    fn age_boundary_changed(&self, _baby: bool) {
        self.refresh_dimensions();
    }
}

impl Animal for SheepEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        SheepEntity::is_food(item_stack)
    }

    fn initialize_breed_offspring(&self, partner: &dyn Animal, offspring: &dyn Animal) {
        let parent1_color = self.color();
        let parent2_color = partner
            .downcast_ref::<SheepEntity>()
            .map_or(parent1_color, SheepEntity::color);
        let mixed_color = SheepEntity::get_mixed_color(parent1_color, parent2_color);
        if let Some(offspring) = offspring.downcast_ref::<SheepEntity>() {
            offspring.set_color(mixed_color);
        }
    }
}

impl Mob for SheepEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn custom_server_ai_step(&self) {
        Animal::custom_server_ai_step_animal(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_SHEEP_AMBIENT)
    }

    fn ate(&self) {
        // VANILLA CLIENT-LOCAL: `LivingEntity.ate` only spawns the eating particles.
        self.set_sheared(false);
        if self.can_age_up() {
            self.age_up(ATE_AGE_UP_SECONDS, false);
        }
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let mut random = LegacyRandom::from_seed(rand::random());
        let color = match world.biome_at(self.block_position()) {
            Some(biome) => SheepEntity::random_sheep_color(biome, &mut random),
            None => SheepEntity::pick_spawn_color(TEMPERATE_SPAWN_COLORS, &mut random),
        };
        self.set_color(color);

        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.copy_with_count(item_stack.count())
        };

        if item_stack.is(&vanilla_items::SHEARS) {
            if !self.ready_for_shearing() {
                return InteractionResult::Consume;
            }
            let Some(world) = self.level() else {
                return InteractionResult::Consume;
            };
            self.shear(world.as_ref(), &item_stack);
            // Vanilla `Sheep.mobInteract` sources the shear game event to the player.
            self.game_event_with_source_entity(&vanilla_game_events::SHEAR, Some(player));
            player
                .inventory
                .lock()
                .hurt_item_in_hand(hand, 1, player.has_infinite_materials());
            return InteractionResult::SuccessServer;
        }

        Animal::mob_interact_animal(self, player, hand)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl PathfinderMob for SheepEntity {}

#[cfg(test)]
mod tests;
