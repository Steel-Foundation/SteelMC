use crate::{
    entity::{
        Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity, LivingEntityBase, Mob,
        MobBase, PathfinderMob,
        ai::{
            goal::{
                BlazeAttackGoal, HurtByTargetGoal, LookAtPlayerGoal, MoveTowardsRestrictionGoal,
                NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
            },
            path::PathType,
        },
        damage::DamageSource,
        monster::Monster,
    },
    physics::MoveResult,
    world::World,
};
use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    entity_type::EntityTypeRef, sound_event::SoundEventRef, sound_events,
    vanilla_entity_data::BlazeEntityData,
};
use steel_utils::{DowncastType, DowncastTypeKey, locks::SyncMutex, random::triangle_random};

#[entity_behavior(class = "Blaze")]
/// Vanilla blaze entity.
pub struct BlazeEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<BlazeEntityData>,
    height_data: SyncMutex<BlazeHeightData>,
}

pub struct BlazeHeightData {
    allowed_offset: f64,
    next_offset_change_tick: i8,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BlazeEntity`.
unsafe impl DowncastType for BlazeEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/blaze");
}

impl BlazeEntity {
    /// Creates a new cow at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a cow from persisted base entity state.
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
        let mut entity_data = BlazeEntityData::new();
        let height_data = BlazeHeightData {
            allowed_offset: 0.5,
            next_offset_change_tick: 0,
        };

        {
            let mut malus = mob_base.pathfinding_malus().lock();
            malus.set(PathType::Water, -1.0);
            malus.set(PathType::Lava, 8.0);
            malus.set(PathType::FireInNeighbor, 0.0);
            malus.set(PathType::Fire, 0.0);
        }

        // TODO: Change the xp reward to 10

        living_base.initialize_synced_data(&mut entity_data);

        {
            // Keep vanilla AbstractCow goal priorities and speeds in the same order.
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(4, BlazeAttackGoal::new());
            goal_selector.add_goal(5, MoveTowardsRestrictionGoal::new(1.0));
            goal_selector.add_goal(7, WaterAvoidingRandomStrollGoal::with_probability(1.0, 0.0));
            goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());
            goal_selector.add_goal(1, HurtByTargetGoal::new().set_alert_others(None));
            goal_selector.add_goal(
                2,
                NearestAttackableTargetGoal::new_for_players(true, |_, _| true),
            );
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            height_data: SyncMutex::new(height_data),
        }
    }

    /// Returns the charged data flag
    pub fn is_charged(&self) -> bool {
        self.entity_data.lock().flags.get() & 1 != 0
    }

    /// Sets the charged data flag
    pub fn set_charged(&self, value: bool) {
        let mut data = self.entity_data.lock();
        let current_value = *data.flags.get();
        if value {
            data.flags.set(current_value | 1);
        } else {
            data.flags.set(current_value & !1);
        }
    }
}

impl Monster for BlazeEntity {}

impl Entity for BlazeEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn sound_source(&self) -> SoundSource {
        Monster::monster_sound_source(self)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
    }

    fn is_on_fire(&self) -> bool {
        self.is_charged()
    }

    fn light_level_dependent_magic_value(&self) -> f32 {
        1.0
    }
}

impl LivingEntity for BlazeEntity {
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

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_BLAZE_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_BLAZE_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();

        if !self.on_ground() && self.velocity().y < 0.0 {
            self.set_velocity(self.velocity() * DVec3::new(1.0, 0.6, 1.0));
        }

        Monster::monster_ai_step(self);
        result
    }
}

impl Mob for BlazeEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_BLAZE_AMBIENT)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    fn custom_server_ai_step(&self) {
        let mut height_data = self.height_data.lock();
        height_data.next_offset_change_tick -= 1;
        if height_data.next_offset_change_tick <= 0 {
            height_data.next_offset_change_tick = 100;
            height_data.allowed_offset = triangle_random(0.5, 6.891);
        }

        let target = self.target();

        if let Some(target) = target.as_deref().and_then(Entity::as_living_entity)
            && target.get_eye_y() > self.get_eye_y() + height_data.allowed_offset
            && Mob::can_attack(self, target)
        {
            let vel = self.velocity();
            self.set_velocity(vel + DVec3::new(0.0, 0.3 - vel.y, 0.0));
            self.mark_velocity_sync();
        }
    }
}

impl PathfinderMob for BlazeEntity {}
