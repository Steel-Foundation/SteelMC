//! Villager entity implementation.

use std::str::FromStr;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_macros::{entity_behavior, entity_impl};
use steel_protocol::packets::game::{
    AttributeSnapshot, CMerchantOffers, EquipmentSlotItem, MerchantOfferData, SoundSource,
};
use steel_registry::entity_data::VillagerData;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::VillagerEntityData;
use steel_registry::{
    REGISTRY, RegistryEntry, RegistryExt, sound_events, vanilla_attributes, vanilla_particle_types,
};
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use text_components::TextComponent;

use crate::behavior::InteractionResult;
use crate::entity::ai::brain::{
    AcquireBed, AcquireJobSite, Activity, AssignProfession, Brain, LookAtTargetSink,
    MemoryModuleType, MoveToTargetSink, NearestLivingEntitiesSensor, RandomStroll, Schedule,
    SetEntityLookTarget, SetWalkTargetFromHome,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason,
    EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase, MobEffectSyncChange,
    PathfinderMob, SharedEntity, SpawnGroupData, Villager,
};
use crate::inventory::MerchantMenuProvider;
use crate::physics::MoveResult;
use crate::player::Player;
use crate::trading::{MerchantOffer, SharedMerchantOffers, offers_for};
use crate::world::World;

const VILLAGER_BABY_SCALE: f32 = 0.5;

const VILLAGER_DEFAULT_SCHEDULE: Schedule = Schedule::new(&[
    (10, Activity::Idle),
    (2000, Activity::Work),
    (9000, Activity::Meet),
    (11000, Activity::Idle),
    (12000, Activity::Rest),
]);

#[entity_behavior(class = "Villager")]
pub struct VillagerEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    entity_data: SyncMutex<VillagerEntityData>,
    brain: SyncMutex<Brain>,
    offers: SharedMerchantOffers,
    villager_xp: SyncMutex<i32>,
    trading_player: SyncMutex<Option<i32>>,
    trade_state: SyncMutex<TradeState>,
}

#[derive(Default)]
struct TradeState {
    update_merchant_timer: i32,
    increase_level_pending: bool,
    last_restock_game_time: i64,
    restocks_today: i32,
    last_restock_check_day: i64,
}

impl VillagerEntity {
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

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
        let mut entity_data = VillagerEntityData::new();

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            entity_data: SyncMutex::new(entity_data),
            brain: SyncMutex::new(Self::make_brain()),
            offers: Arc::new(SyncMutex::new(Vec::new())),
            villager_xp: SyncMutex::new(0),
            trading_player: SyncMutex::new(None),
            trade_state: SyncMutex::new(TradeState::default()),
        }
    }

    #[must_use]
    pub fn villager_data(&self) -> VillagerData {
        *self.entity_data.lock().villager_data.get()
    }

    pub fn set_villager_data(&self, data: VillagerData) {
        self.entity_data.lock().villager_data.set(data);
    }

    #[must_use]
    pub fn get_age(&self) -> i32 {
        AgeableMob::get_age(self)
    }

    pub fn set_age(&self, age: i32) {
        AgeableMob::set_age(self, age);
    }

    #[must_use]
    pub fn is_baby(&self) -> bool {
        AgeableMob::is_baby(self)
    }

    pub fn set_baby(&self, baby: bool) {
        AgeableMob::set_baby(self, baby);
    }

    fn start_trading(&self, player: &Player) {
        let Some(world) = self.level() else { return };
        let Some(merchant) = world.get_entity_by_id(self.id()) else {
            return;
        };

        self.set_trading_player(Some(player.id()));

        let title = self
            .custom_name()
            .unwrap_or_else(|| TextComponent::translated("entity.minecraft.vilager"));
        let provider =
            MerchantMenuProvider::new(player.inventory.clone(), self.offers(), merchant, title);
        let container_id = player.open_menu(&provider);

        let data = self.villager_data();
        let offers: Vec<MerchantOfferData> =
            self.offers().lock().iter().map(offer_to_packet).collect();
        if offers.is_empty() {
            return;
        }
        player.send_packet(CMerchantOffers {
            container_id: i32::from(container_id),
            offers,
            villager_level: data.level,
            villager_xp: self.villager_xp(),
            show_progress: true,
            can_restock: true,
        });
    }

    fn should_increase_level(&self) -> bool {
        let data = self.villager_data();
        can_level_up(data.level) && self.villager_xp() >= max_xp_per_level(data.level)
    }

    fn increase_merchant_career(&self) {
        let mut data = self.villager_data();
        data.level += 1;
        self.set_villager_data(data);
        self.updateTrades();
        self.resend_offers_to_trading_player();
    }
    
    fn should_restock(&self) -> bool {
        let Some(world) = self.level() else { return false };
        let game_time = world.game_time();

        let (allowed, reset_uses_on_new_day) = {
            let mut state = self.trade_state.lock();
            let current_day = game_time / 24000;
            let mut is_new_day = game_time > state.last_restock_game_time + 12000;
            is_new_day |= state.last_restock_check_day > 0 && current_day > state.last_restock_check_day;
            state.last_restock_check_day = current_day;

            let mut reset = false;
            if is_new_day {
                state.last_restock_game_time = game_time;
                reset = (2 - state.restocks_today) > 0;
                state.restocks_today = 0;
            }
            let allowed = state.restocks_today == 0
                || (state.restocks_today < 2 && game_time > state.last_restock_game_time + 2400);
            (allowed, reset)
        };

        if reset_uses_on_new_day {
            for offer in self.offers.lock().iter_mut() {
                offer.reset_uses();
            }
            self.resend_offers_to_trading_player();
        }
        allowed && self.needs_to_restock()
    }

    fn needs_to_restock(&self) -> bool {
        self.offers.lock().iter().any(MerchantOffer::needs_restock)
    }

    fn restock(&self) {
        for offer in self.offers.lock().iter_mut() {
            offer.reset_uses();
        }
        self.resend_offers_to_trading_player();
        if let Some(world) = self.level() {
            let mut state = self.trade_state.lock();
            state.last_restock_game_time = world.game_time();
            state.restocks_today += 1;
        }
    }

    fn resend_offers_to_trading_player(&self) {
        //TODO resend CMerchantOffers to a player if currently trading. This needs the open menu's
        //id. Store it in TradeState.
    }
}

#[entity_impl(class(ageable_mob))]
impl Entity for VillagerEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.is_baby() {
            self.entity_type
                .dimensions
                .scale(VILLAGER_BABY_SCALE * scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn tick(&self) {
        self.default_tick();
        self.living_base.decrement_invulnerable_time();
        self.tick_mob_effects();

        if self.is_dead_or_dying() {
            LivingEntity::tick_death(self);
            self.tick_living_state();
            return;
        }

        if !self.is_removed() {
            self.ai_step();
        }

        self.tick_living_state();
    }

    fn check_despawn(&self) {
        Mob::check_mob_despawn(self);
    }

    fn is_alive(&self) -> bool {
        !self.is_removed() && self.get_health() > 0.0
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_pushable(&self) -> bool {
        Entity::is_alive(self) && !self.is_spectator() && !self.on_climbable()
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        self.controlling_passenger_mob()
    }

    fn is_effective_ai(&self) -> bool {
        self.is_server_driven_movement() && !self.is_no_ai()
    }

    fn get_default_gravity(&self) -> f64 {
        LivingEntity::get_attribute_gravity(self)
    }

    fn can_freeze(&self) -> bool {
        self.default_living_can_freeze()
    }

    fn can_walk_on_powder_snow(&self) -> bool {
        self.default_living_can_walk_on_powder_snow()
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().syncable_snapshots()
    }

    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().drain_dirty_sync()
    }

    fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        self.living_base.drain_dirty_mob_effects()
    }

    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        self.pack_living_equipment()
    }

    fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
        self.drain_dirty_living_equipment()
    }

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(0.6) as f32
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        LivingEntity::hurt_server(self, source, amount)
    }

    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        Mob::interact_mob(self, player, hand, location)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);

        let data = self.villager_data();
        let mut villager_data = NbtCompound::new();
        if let Some(villager_type) = usize::try_from(data.villager_type)
            .ok()
            .and_then(|id| REGISTRY.villager_types.by_id(id))
        {
            villager_data.insert("type", villager_type.key.to_string());
        }
        if let Some(profession) = usize::try_from(data.profession)
            .ok()
            .and_then(|id| REGISTRY.villager_professions.by_id(id))
        {
            villager_data.insert("profession", profession.key.to_string());
        }
        villager_data.insert("level", data.level);
        nbt.insert("VillagerData", NbtTag::Compound(villager_data));
        nbt.insert("Xp", self.villager_xp());
        let state = self.trade_state.lock();
        nbt.insert("LastRestock", state.last_restock_game_time);
        nbt.insert("RestocksToday", state.restocks_today);
        let offers = self.offers.lock();
        if !offers.is_empty() {
            let recepies: Vec<NbtCompound> = offers.iter().map(MerchantOffer::to_nbt).collect();
            nbt.insert("Offers", NbtList::Compound(recepies));
        }
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);

        if let Some(villager_data) = nbt.compound("VillagerData") {
            let mut data = self.villager_data();
            if let Some(type_key) = villager_data.string("type")
                && let Ok(key) = Identifier::from_str(type_key.to_str().as_ref())
                && let Some(id) = REGISTRY.villager_types.id_from_key(&key)
            {
                data.villager_type = i32::try_from(id).unwrap_or(data.villager_type);
            }
            if let Some(profession_key) = villager_data.string("profession")
                && let Ok(key) = Identifier::from_str(profession_key.to_str().as_ref())
                && let Some(id) = REGISTRY.villager_professions.id_from_key(&key)
            {
                data.profession = i32::try_from(id).unwrap_or(data.profession);
            }
            if let Some(level) = villager_data.int("level") {
                data.level = level;
            }
            if let Some(xp) = nbt.int("Xp") {
                *self.villager_xp.lock() = xp;
            }
            if let Some(v) = nbt.long("LastRestock") {
                self.trade_state.lock().last_restock_game_time = v;
            }
            if let Some(v) = nbt.int("RestocksToday") {
                self.trade_state.lock().restocks_today = v;
            }
            let mut loaded_offers = false;
            if let Some(list) = nbt.list("Offers")
                && let Some(compounds) = list.compounds()
            {
                let mut offers = self.offers.lock();
                offers.clear();
                for compound in compounds {
                    if let Some(offer) = MerchantOffer::from_nbt(&compound) {
                        offers.push(offer);
                    }
                }
                loaded_offers = true;
            }
            if !loaded_offers {
                self.updateTrades();
            }
            self.set_villager_data(data);
        }
    }
}

impl VillagerEntity {
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }

        let Some(particle_type_id) = vanilla_particle_types::ENTITY_EFFECT.try_id() else {
            log::error!("vanilla entity_effect particle type is not registered");
            return;
        };
        let Ok(particle_type_id) = i32::try_from(particle_type_id) else {
            log::error!("vanilla entity_effect particle type id does not fit protocol i32");
            return;
        };
        let display = self.living_base.mob_effect_display_state(particle_type_id);

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

    fn make_brain() -> Brain {
        let mut brain = Brain::new(
            [
                MemoryModuleType::LookTarget,
                MemoryModuleType::WalkTarget,
                MemoryModuleType::Home,
                MemoryModuleType::JobSite,
            ],
            vec![Box::new(NearestLivingEntitiesSensor)],
        );
        brain.set_core_activities([Activity::Core]);
        brain.add_activity(
            Activity::Core,
            0,
            vec![
                Box::new(MoveToTargetSink::new(150, 250)),
                Box::new(LookAtTargetSink::new(45, 90)),
                Box::new(AcquireBed::new(48)),
                Box::new(AcquireJobSite::new(48)),
                Box::new(AssignProfession::new()),
            ],
        );
        brain.add_activity(
            Activity::Idle,
            0,
            vec![
                Box::new(RandomStroll::new(0.5)),
                Box::new(SetEntityLookTarget::new(
                    |entity| entity.as_player().is_some(),
                    8.0,
                )),
            ],
        );
        brain.add_activity(
            Activity::Rest,
            0,
            vec![Box::new(SetWalkTargetFromHome::new(0.6, 1))],
        );
        brain.add_activity(Activity::Work, 0, vec![
            Box::new(SetWalkTargetFromJobSite::new(0.5, 1)),
            Box::new(WorkAtPoi::new())
        ]);
        brain.add_activity(Activity::Meet, 0, Vec::new());
        brain.set_schedule(VILLAGER_DEFAULT_SCHEDULE);
        brain.use_default_activity();
        brain
    }
}

impl LivingEntity for VillagerEntity {
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

    fn is_baby(&self) -> bool {
        AgeableMob::is_baby(self)
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();
        AgeableMob::tick_ageable_mob(self);
        result
    }
}

impl AgeableMob for VillagerEntity {
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

impl Mob for VillagerEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn as_villager(&self) -> Option<&dyn Villager> {
        Some(self)
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_AMBIENT)
    }

    fn remove_when_far_away(&self, _dist_sqr: f64) -> bool {
        false
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    fn custom_server_ai_step(&self) {
        let (game_time, day_time) = self
            .level()
            .map_or((0, 0), |world| (world.game_time(), world.day_time()));
        let mut brain = self.brain.lock();
        brain.update_activity_from_schedule(game_time, day_time);
        brain.tick(self, game_time);
        drop(brain);
        if !self.is_trading() {
            let mut fire_career = false;
            {
                let mut state = self.trade_state.lock();
                if state.update_merchant_timer > 0 {
                    state.update_merchant_timer -= 1;
                    if state.update_merchant_timer == 0 {
                        fire_career = std::mem::take(&mut state.increase_level_pending);
                    }
                }
            }
            if fire_career {
                self.increase_merchant_career();
            }
        }
    }

    fn mob_interact(&self, player: &Player, _hand: InteractionHand) -> InteractionResult {
        if !Entity::is_alive(self) || self.is_trading() || self.is_sleeping() {
            return InteractionResult::Pass;
        }
        if self.is_baby() {
            //TODO: setUnhappy
            return InteractionResult::Success;
        }
        if self.offers().lock().is_empty() {
            //TODO: setUnhappy on main hand and award TALKED_TO_VILLAGER stat
            return InteractionResult::ConsumeM;
        }
        self.start_trading(player);
        InteractionResult::Success
    }
}

impl PathfinderMob for VillagerEntity {}

impl Villager for VillagerEntity {
    fn villager_data(&self) -> VillagerData {
        *self.entity_data.lock().villager_data.get()
    }

    fn set_villager_data(&self, data: VillagerData) {
        self.entity_data.lock().villager_data.set(data);
    }

    fn offers(&self) -> SharedMerchantOffers {
        Arc::clone(&self.offers)
    }

    fn updateTrades(&self) {
        let data = self.villager_data();
        let Some(profession) = usize::try_from(data.profession)
            .ok()
            .and_then(|id| REGISTRY.villager_professions.by_id(id))
        else {
            return;
        };
        let new_offers = offers_for(&profession.key, data.level);
        self.offers.lock().extend(new_offers);
    }

    fn villager_xp(&self) -> i32 {
        *self.villager_xp.lock()
    }

    fn notify_trade(&self, xp: i32) {
        *self.villager_xp.lock() += xp;
        if self.should_increase_level() {
            let mut state = self.trade_state.lock();
            state.update_merchant_timer = 40;
            state.increase_level_pending = true;
        }
        // TODO spawn ExperienceOrb, play ENTITY_VILLAGER_YES, set lastTradedPlayer for the TRADE
        // reputation event, happy particle.
    }

    fn is_trading(&self) -> bool {
        self.trading_player.lock().is_some()
    }

    fn set_trading_player(&self, id: Option<i32>) {
        *self.trading_player.lock() = id;
    }
    fn try_restock(&self) {
        if self.should_restock() {
            self.restock();
        }
    }
}

fn offer_to_packet(offer: &MerchantOffer) -> MerchantOfferData {
    MerchantOfferData {
        cost_a: offer.cost_a().clone(),
        result: offer.result().clone(),
        cost_b: offer.cost_b().cloned(),
        out_of_stock: offer.is_out_of_stock(),
        uses: offer.uses(),
        max_uses: offer.max_uses(),
        xp: offer.xp(),
        special_price: 0,
        price_multiplier: 0.0,
        demand: 0,
    }
}

const MAX_VILLAGER_LEVEL: i32 = 5;

const fn can_level_up(level: i32) -> bool {
    level >= 1 && level < MAX_VILLAGER_LEVEL
}

const fn max_xp_per_level(level: i32) -> i32 {
    match level {
        1 => 10,
        2 => 70,
        3 => 150,
        4 => 250,
        _ => 0,
    }
}
