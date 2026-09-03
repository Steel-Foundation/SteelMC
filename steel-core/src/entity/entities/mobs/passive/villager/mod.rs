//! Vanilla Villager entity — ported from Pumpkin (foundation-first).

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::VillagerEntityData;
use steel_registry::vanilla_mob_effects;
use steel_registry::{REGISTRY, RegistryExt};
use steel_registry::{sound_events, vanilla_custom_stats};
use steel_utils::locks::SyncMutex;
use steel_utils::random::xoroshiro::Xoroshiro;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Downcast, DowncastType, DowncastTypeKey};
use uuid::Uuid;

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    AvoidEntityGoal, FloatGoal, HurtByTargetGoal, LookAtPlayerGoal, PanicGoal,
    RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData,
    LivingEntity, LivingEntityBase, Mob, MobBase, MobEffectInstance, PathfinderMob, SpawnGroupData,
};
use crate::inventory::menu::kinds::{MerchantAccess, merchant};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::villager::{
    MAX_VILLAGER_LEVEL, MerchantOffer, can_level_up, max_xp_per_level, offers_from_random,
};
use crate::world::World;

mod job_site;

use job_site::JobSiteMemories;

#[entity_behavior(class = "Villager")]
/// Entity behavior for the villager.
pub struct VillagerEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<VillagerEntityData>,
    offers: Arc<SyncMutex<Vec<MerchantOffer>>>,
    merchant_state: SyncMutex<VillagerMerchantState>,
    job_site_memories: SyncMutex<JobSiteMemories>,
}

/// Runtime merchant career state (vanilla `villagerXp` / `updateMerchantTimer`).
struct VillagerMerchantState {
    xp: i32,
    update_merchant_timer: i32,
    increase_level_on_update: bool,
    trading_player: Option<Uuid>,
}

impl Default for VillagerMerchantState {
    fn default() -> Self {
        Self {
            xp: 0,
            update_merchant_timer: 0,
            increase_level_on_update: false,
            trading_player: None,
        }
    }
}

/// Menu-facing handle that looks the villager up by id so the menu does not hold `Arc<Self>`.
struct VillagerMerchantAccess {
    world: Weak<World>,
    villager_id: i32,
    offers: Arc<SyncMutex<Vec<MerchantOffer>>>,
}

// SAFETY: key is owner-scoped to this Steel entity type; the implementation only
// exposes `VillagerEntity` itself.
unsafe impl DowncastType for VillagerEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/villager");
}

fn is_hostile_entity(target: &dyn LivingEntity, _world: &World) -> bool {
    let key = &target.entity_type().key;
    *key == vanilla_entities::ZOMBIE.key
        || *key == vanilla_entities::ZOMBIE_VILLAGER.key
        || *key == vanilla_entities::ZOMBIFIED_PIGLIN.key
        || *key == vanilla_entities::HUSK.key
        || *key == vanilla_entities::DROWNED.key
        || *key == vanilla_entities::VINDICATOR.key
        || *key == vanilla_entities::EVOKER.key
        || *key == vanilla_entities::PILLAGER.key
        || *key == vanilla_entities::ILLUSIONER.key
        || *key == vanilla_entities::RAVAGER.key
        || *key == vanilla_entities::VEX.key
}

/// Resolves a villager profession registry id to its trade-set name (the registry path),
/// e.g. the id for `minecraft:farmer` resolves to `"farmer"`.
fn profession_name(id: i32) -> Option<&'static str> {
    let id = usize::try_from(id).ok()?;
    let profession = REGISTRY.villager_professions.by_id(id)?;
    Some(profession.key.path.as_ref())
}

/// Reads a flat `{X, Y, Z}` int-array position written by `save_additional`.
fn load_block_pos(nbt: BorrowedNbtCompoundView<'_, '_>, key: &str) -> Option<BlockPos> {
    let position = nbt.int_array(key).filter(|position| position.len() == 3)?;
    Some(BlockPos::new(position[0], position[1], position[2]))
}

impl VillagerEntity {
    #[must_use]
    /// Creates a new instance.
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }
    #[must_use]
    /// Creates an instance from saved data.
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }
    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mob_base = MobBase::new();
        let mut entity_data = VillagerEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(1.5));
            goal_selector.add_goal(
                2,
                AvoidEntityGoal::with_selector(8.0, 0.6, 0.6, |target, world| {
                    is_hostile_entity(target, world)
                }),
            );
            goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(0.5));
            goal_selector.add_goal(6, LookAtPlayerGoal::new(6.0));
            goal_selector.add_goal(7, RandomLookAroundGoal::new());
            // Vanilla core-package job-site goals (see `job_site` module docs for the
            // priority mapping against vanilla brain priorities).
            job_site::register_job_site_goals(&mut goal_selector);
        }
        {
            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new());
        }
        let villager = Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            offers: Arc::new(SyncMutex::new(Vec::new())),
            merchant_state: SyncMutex::new(VillagerMerchantState::default()),
            job_site_memories: SyncMutex::new(JobSiteMemories::default()),
        };
        villager.update_trades();
        villager
    }

    /// Vanilla `Villager.updateTrades`: appends offers for the current career level.
    ///
    /// A villager with no profession (`minecraft:none`) or a nitwit has no trade sets, so it
    /// ends up with no offers and cannot trade. Profession changes discard existing offers
    /// first; level-ups keep earlier trades and add the new tier.
    fn update_trades(&self) {
        let (profession, tier) = {
            let data = self.entity_data.lock();
            let villager_data = data.villager_data.get();
            (
                villager_data.profession,
                villager_data.level.clamp(1, MAX_VILLAGER_LEVEL) as u8,
            )
        };
        let Some(name) = profession_name(profession) else {
            return;
        };
        // Vanilla selects offers with the entity's unseeded `random`, so each
        // profession (re)assignment rolls fresh trades. Incidental gameplay
        // randomness, so Steel's unseeded runtime RNG applies.
        let mut random = Xoroshiro::from_seed(rand::random::<u64>());
        let added = offers_from_random(name, tier, &mut random);
        self.offers.lock().extend(added);
    }

    fn villager_level(&self) -> i32 {
        self.entity_data.lock().villager_data.get().level
    }

    pub(crate) fn villager_xp(&self) -> i32 {
        self.merchant_state.lock().xp
    }

    fn is_trading(&self) -> bool {
        self.merchant_state.lock().trading_player.is_some()
    }

    fn set_trading_player(&self, player: Option<Uuid>) {
        self.merchant_state.lock().trading_player = player;
    }

    fn stop_trading(&self) {
        self.set_trading_player(None);
    }

    fn merchant_access(&self) -> Arc<VillagerMerchantAccess> {
        Arc::new(VillagerMerchantAccess {
            world: self
                .level()
                .as_ref()
                .map(Arc::downgrade)
                .unwrap_or_else(Weak::new),
            villager_id: self.id(),
            offers: Arc::clone(&self.offers),
        })
    }

    /// Vanilla `Villager.notifyTrade` / `rewardTradeXp`.
    fn notify_trade(&self, player: &Player, offer_xp: i32) {
        self.play_sound(
            &sound_events::ENTITY_VILLAGER_YES,
            self.sound_volume(),
            self.voice_pitch(),
        );
        player.award_custom_stat(&vanilla_custom_stats::TRADED_WITH_VILLAGER);
        self.reward_trade_xp(offer_xp);
    }

    fn reward_trade_xp(&self, offer_xp: i32) {
        let level = self.villager_level();
        let mut orb_xp = 3 + rand::random_range(0..4);
        {
            let mut state = self.merchant_state.lock();
            state.xp = state.xp.saturating_add(offer_xp.max(0));
            if can_level_up(level) && state.xp >= max_xp_per_level(level) {
                state.update_merchant_timer = 40;
                state.increase_level_on_update = true;
                orb_xp += 5;
            }
        }
        let Some(world) = self.level() else {
            return;
        };
        let pos = self.position();
        ExperienceOrbEntity::award(&world, DVec3::new(pos.x, pos.y + 0.5, pos.z), orb_xp);
    }

    /// Vanilla `Villager.customServerAiStep` merchant-timer branch.
    ///
    /// The career only advances after the trading screen is closed.
    fn tick_merchant_career(&self) {
        let should_level = {
            let mut state = self.merchant_state.lock();
            if state.trading_player.is_some() || state.update_merchant_timer <= 0 {
                return;
            }
            state.update_merchant_timer -= 1;
            if state.update_merchant_timer > 0 {
                return;
            }
            let increase = state.increase_level_on_update;
            state.increase_level_on_update = false;
            increase
        };
        if should_level {
            self.increase_merchant_career();
        }
        self.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::REGENERATION,
            200,
            0,
        ));
    }

    fn increase_merchant_career(&self) {
        {
            let mut data = self.entity_data.lock();
            let mut villager_data = *data.villager_data.get();
            if villager_data.level >= MAX_VILLAGER_LEVEL {
                return;
            }
            villager_data.level += 1;
            data.villager_data.set(villager_data);
        }
        self.update_trades();
    }

    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }
        let display = self.living_base.mob_effect_display_state();
        {
            let mut d = self.entity_data.lock();
            let l = d.living_entity_mut();
            l.effect_particles.set(display.particles);
            l.effect_ambience.set(display.ambient);
        }
        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }
}

impl Entity for VillagerEntity {
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
        let s = LivingEntity::get_scale(self);
        if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(s)
        }
    }
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }
    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }
    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }
    fn play_step_sound(&self, _pos: BlockPos, _block: BlockStateId) {
        self.play_sound(&sound_events::ENTITY_VILLAGER_WORK_ARMORER, 0.15, 1.0);
    }
    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        let data = self.entity_data.lock();
        let vd = data.villager_data.get();
        nbt.insert("Profession", vd.profession);
        nbt.insert("Type", vd.villager_type);
        nbt.insert("Level", vd.level);
        drop(data);
        nbt.insert("Xp", self.villager_xp());
        // Vanilla persists the job-site brain memories; store them as flat int arrays
        // (Steel convention, cf. `home_pos`).
        let memories = self.job_site_memories.lock();
        if let Some(pos) = memories.job_site {
            nbt.insert("JobSite", NbtTag::IntArray(vec![pos.x(), pos.y(), pos.z()]));
        }
        if let Some(pos) = memories.potential_job_site {
            nbt.insert(
                "PotentialJobSite",
                NbtTag::IntArray(vec![pos.x(), pos.y(), pos.z()]),
            );
        }
        drop(memories);

        // Vanilla persists offers under `Offers.Recipes`; without this, loading would
        // re-roll trades.
        let offers = self.offers.lock();
        if !offers.is_empty() {
            let recipes = offers
                .iter()
                .map(|offer| {
                    let mut recipe = NbtCompound::new();
                    recipe.insert("buy", offer.cost_a.to_nbt_tag_ref());
                    if let Some(cost_b) = &offer.cost_b {
                        recipe.insert("buyB", cost_b.to_nbt_tag_ref());
                    }
                    recipe.insert("sell", offer.result.to_nbt_tag_ref());
                    recipe.insert("uses", offer.uses);
                    recipe.insert("maxUses", offer.max_uses);
                    recipe.insert("xp", offer.xp);
                    recipe.insert("priceMultiplier", offer.reputation_discount);
                    recipe
                })
                .collect();
            let mut offers_compound = NbtCompound::new();
            offers_compound.insert("Recipes", NbtTag::List(NbtList::Compound(recipes)));
            nbt.insert("Offers", offers_compound);
        }
    }
    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        {
            let mut data = self.entity_data.lock();
            let mut vd = *data.villager_data.get();
            vd.profession = nbt.int("Profession").unwrap_or(0);
            vd.villager_type = nbt.int("Type").unwrap_or(0);
            vd.level = nbt.int("Level").unwrap_or(1);
            data.villager_data.set(vd);
        }
        self.merchant_state.lock().xp = nbt.int("Xp").unwrap_or(0).max(0);
        // Memories restore blindly; validation re-checks them in world (vanilla-faithful:
        // persisted memories keep the profession across reloads).
        {
            let mut memories = self.job_site_memories.lock();
            memories.job_site = load_block_pos(nbt, "JobSite");
            memories.potential_job_site = load_block_pos(nbt, "PotentialJobSite");
        }
        match load_offers(nbt) {
            Some(offers) => *self.offers.lock() = offers,
            // Fresh villagers (and saves predating offer persistence) roll trades.
            None => self.update_trades(),
        }
    }
}

/// Parses vanilla's `Offers.Recipes` list. Returns `None` when the tag is absent or
/// malformed so the caller falls back to rolling fresh offers.
fn load_offers(nbt: BorrowedNbtCompoundView<'_, '_>) -> Option<Vec<MerchantOffer>> {
    let recipes = nbt.compound("Offers")?.list("Recipes")?;
    if recipes.empty() {
        return Some(Vec::new());
    }
    recipes.compounds()?.into_iter().map(load_offer).collect()
}

fn load_offer(recipe: BorrowedNbtCompoundView<'_, '_>) -> Option<MerchantOffer> {
    let cost_a = ItemStack::from_borrowed_compound(&recipe.compound("buy")?)?;
    let cost_b = recipe
        .compound("buyB")
        .and_then(|cost| ItemStack::from_borrowed_compound(&cost));
    let result = ItemStack::from_borrowed_compound(&recipe.compound("sell")?)?;
    let max_uses = recipe.int("maxUses")?.max(1);
    let uses = recipe.int("uses").unwrap_or(0).clamp(0, max_uses);
    let xp = recipe.int("xp").unwrap_or(0).max(0);
    let reputation_discount = recipe.float("priceMultiplier").unwrap_or(0.0).max(0.0);
    Some(MerchantOffer {
        cost_a,
        cost_b,
        result,
        max_uses,
        xp,
        reputation_discount,
        uses,
    })
}

impl LivingEntity for VillagerEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }
    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }
    fn set_health(&self, h: f32) {
        let m = self.get_max_health();
        let c = h.clamp(0.0, m);
        self.entity_data.lock().living_entity_mut().health.set(c);
    }
    fn sound_volume(&self) -> f32 {
        0.4
    }
    fn hurt_sound(&self, _s: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_HURT)
    }
    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_DEATH)
    }
    fn server_ai_step(&self) {
        self.tick_merchant_career();
        Mob::mob_server_ai_step(self);
    }
    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();

        // Vanilla `Villager.tick`: the unhappy head-shake decays over 40 ticks.
        {
            let mut data = self.entity_data.lock();
            let counter = *data.abstract_villager().unhappy_counter.get();
            if counter > 0 {
                data.abstract_villager_mut()
                    .unhappy_counter
                    .set(counter - 1);
            }
        }
        result
    }
    /// Vanilla `Villager.die`: release the held POI tickets before the living death
    /// processing.
    fn die(&self, source: &DamageSource) {
        self.stop_trading();
        self.release_job_site_pois();
        self.die_living_entity(source);
    }
}

impl Mob for VillagerEntity {
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
        Some(&sound_events::ENTITY_VILLAGER_AMBIENT)
    }
    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        r: EntitySpawnReason,
        g: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, r, g)
    }
    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }
    fn set_mob_flags(&self, f: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(f);
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        // Vanilla `Villager.mobInteract`: dead/sleeping/already-trading villagers and
        // sneaking players fall through to `super` (Pass here; the spawn-egg branch is
        // item-on-entity behavior Steel does not have yet), babies refuse through
        // `setUnhappy`, and everyone else trades — main hand interactions with no
        // offers get `setUnhappy` + `CONSUME`.
        if !LivingEntity::is_alive(self)
            || LivingEntity::is_sleeping(self)
            || self.is_trading()
            || player.is_secondary_use_active()
        {
            return InteractionResult::Pass;
        }
        if LivingEntity::is_baby(self) {
            self.set_unhappy();
            return InteractionResult::SuccessServer;
        }

        let no_offers = self.offers.lock().is_empty();
        if hand == InteractionHand::MainHand {
            if no_offers {
                self.set_unhappy();
            }
            player.award_custom_stat(&vanilla_custom_stats::TALKED_TO_VILLAGER);
        }
        if no_offers {
            return InteractionResult::Consume;
        }

        self.start_trading(player);
        InteractionResult::SuccessServer
    }
}

impl VillagerEntity {
    /// Vanilla `Villager.startTrading`.
    fn start_trading(&self, player: &Player) {
        self.set_trading_player(Some(player.uuid()));
        let access = self.merchant_access();
        player.open_menu("Villager", move |context| {
            merchant(
                Arc::clone(&context.player.inventory),
                context.container_id,
                access,
            )
        });
    }

    /// Vanilla `Villager.setUnhappy`: 40 ticks of head shaking plus the "no" sound.
    fn set_unhappy(&self) {
        self.entity_data
            .lock()
            .abstract_villager_mut()
            .unhappy_counter
            .set(40);
        self.play_sound(
            &sound_events::ENTITY_VILLAGER_NO,
            self.sound_volume(),
            self.voice_pitch(),
        );
    }
}

impl PathfinderMob for VillagerEntity {}

#[cfg(test)]
mod trading_tests;

impl VillagerMerchantAccess {
    fn with_villager<R>(&self, f: impl FnOnce(&VillagerEntity) -> R) -> Option<R> {
        let world = self.world.upgrade()?;
        let entity = world.get_entity_by_id(self.villager_id)?;
        let villager = entity.as_ref().downcast_ref::<VillagerEntity>()?;
        Some(f(villager))
    }
}

impl MerchantAccess for VillagerMerchantAccess {
    fn offers(&self) -> Arc<SyncMutex<Vec<MerchantOffer>>> {
        Arc::clone(&self.offers)
    }

    fn villager_xp(&self) -> i32 {
        self.with_villager(VillagerEntity::villager_xp).unwrap_or(0)
    }

    fn villager_level(&self) -> i32 {
        self.with_villager(VillagerEntity::villager_level)
            .unwrap_or(1)
    }

    fn notify_trade(&self, player: &Player, offer_xp: i32) {
        self.with_villager(|villager| villager.notify_trade(player, offer_xp));
    }

    fn stop_trading(&self) {
        self.with_villager(VillagerEntity::stop_trading);
    }

    fn still_valid(&self, player: &Player) -> bool {
        self.with_villager(|villager| {
            LivingEntity::is_alive(villager)
                && villager.merchant_state.lock().trading_player == Some(player.uuid())
        })
        .unwrap_or(false)
    }
}
