//! sample group and mob state before constructing entities

mod equine;
mod groups;
mod strider;
mod variants;

use steel_registry::biome::BiomeRef;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::{items::ItemRef, vanilla_items};
use steel_utils::random::{Random, RandomSource};
use steel_utils::{Downcast as _, DowncastType};

use crate::entity::{AgeableMobGroupData, Entity, MobSpawnState};
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

use self::equine::EquineAttributes;
use super::SpawnSite;
pub(crate) use groups::register_worldgen_spawn_groups;

/// Owns the group state and lends it to the spawn loop for one call.
pub(crate) type SpawnGroupFactory = fn(&mut RunSpawnGroup<'_>);
pub(crate) type RunSpawnGroup<'a> = dyn FnMut(&mut dyn WorldgenSpawnGroup) + 'a;

pub(crate) struct SpawnContext<'a> {
    pub site: SpawnSite,
    pub biome: BiomeRef,
    pub world: &'a World,
    pub random: &'a mut RandomSource,
}

pub(crate) trait WorldgenSpawnGroup {
    fn finalize(&mut self, context: SpawnContext<'_>) -> SpawnState<'_>;
}

pub(crate) struct SpawnState<'a> {
    pub data: SpawnData<'a>,
    pub passenger: Option<SpawnData<'a>>,
}

pub(crate) struct SpawnData<'a> {
    pub entity_type: EntityTypeRef,
    mob: MobSpawnState,
    overrides: SpawnOverrides,
    initialize: Option<&'a SpawnInitializer<'a>>,
}

pub(crate) type SpawnInitializer<'a> = dyn Fn(&dyn Entity) -> Result<(), SpawnInitError> + 'a;
type SpawnInitFn = fn(&dyn Entity) -> Result<(), SpawnInitError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SpawnInitError {
    #[error("factory did not create a mob")]
    ExpectedMob,
    #[error("factory created a different entity implementation")]
    WrongEntityType,
    #[error("worldgen initialization is unavailable for this entity implementation")]
    MissingInitializer,
}

#[derive(Default)]
pub(crate) struct SpawnOverrides {
    pub is_baby: bool,
    equine_attributes: Option<EquineAttributes>,
    pub main_hand_item: Option<ItemRef>,
    pub head_item: Option<ItemRef>,
    pub saddled: bool,
    pub can_pick_up_loot: Option<bool>,
}

impl<'a> SpawnData<'a> {
    pub(crate) fn new(
        entity_type: EntityTypeRef,
        mob: MobSpawnState,
        overrides: SpawnOverrides,
        initialize: Option<&'a SpawnInitializer<'a>>,
    ) -> Self {
        Self {
            entity_type,
            mob,
            overrides,
            initialize,
        }
    }
}

pub(crate) fn typed_initializer<T: Entity + DowncastType>(
    initialize: impl Fn(&T),
) -> impl Fn(&dyn Entity) -> Result<(), SpawnInitError> {
    move |entity| {
        let target = entity
            .downcast_ref::<T>()
            .ok_or(SpawnInitError::WrongEntityType)?;
        initialize(target);

        Ok(())
    }
}

fn unavailable_initializer(_entity: &dyn Entity) -> Result<(), SpawnInitError> {
    Err(SpawnInitError::MissingInitializer)
}

struct Sample<P> {
    overrides: SpawnOverrides,
    initialize: Option<P>,
    passenger: Option<SpawnData<'static>>,
}

impl<P> Sample<P> {
    fn typed(initialize: P) -> Self {
        Self {
            overrides: SpawnOverrides::default(),
            initialize: Some(initialize),
            passenger: None,
        }
    }
}

impl Sample<SpawnInitFn> {
    fn common(overrides: SpawnOverrides) -> Self {
        Self {
            overrides,
            initialize: None,
            passenger: None,
        }
    }

    fn shadow_only(overrides: SpawnOverrides) -> Self {
        Self {
            initialize: Some(unavailable_initializer),
            ..Self::common(overrides)
        }
    }
}

struct AgeableSpawnGroup<S, F, P> {
    ageable: AgeableMobGroupData,
    state: S,
    sample: F,
    pending: Option<P>,
}

impl<S, F, P> WorldgenSpawnGroup for AgeableSpawnGroup<S, F, P>
where
    F: FnMut(&mut S, &mut AgeableMobGroupData, &mut SpawnContext<'_>) -> Sample<P>,
    P: Fn(&dyn Entity) -> Result<(), SpawnInitError>,
{
    fn finalize(&mut self, mut context: SpawnContext<'_>) -> SpawnState<'_> {
        let Sample {
            mut overrides,
            initialize,
            passenger,
        } = (self.sample)(&mut self.state, &mut self.ageable, &mut context);
        self.pending = initialize;

        overrides.is_baby |= self
            .ageable
            .finalize_ageable_spawn(|| context.random.next_f32());

        SpawnState {
            data: SpawnData::new(
                context.site.entity_type,
                MobSpawnState::sample(context.random),
                overrides,
                self.pending
                    .as_ref()
                    .map(|initialize| initialize as &SpawnInitializer<'_>),
            ),
            passenger,
        }
    }
}

fn with_ageable_group<S, F, P>(
    run: &mut RunSpawnGroup<'_>,
    ageable: AgeableMobGroupData,
    state: S,
    sample: F,
) where
    F: FnMut(&mut S, &mut AgeableMobGroupData, &mut SpawnContext<'_>) -> Sample<P>,
    P: Fn(&dyn Entity) -> Result<(), SpawnInitError>,
{
    run(&mut AgeableSpawnGroup {
        ageable,
        state,
        sample,
        pending: None,
    });
}

fn choose<T: Copy>(values: &[T], random: &mut RandomSource) -> T {
    values[random.next_i32_bounded(values.len() as i32) as usize]
}

fn required_variant<T>(variant: Option<T>) -> T {
    match variant {
        Some(variant) => variant,
        None => panic!("worldgen spawn sound variant registry must be populated"),
    }
}

impl SpawnData<'_> {
    pub(super) fn apply(&self, entity: &dyn Entity) -> Result<(), SpawnInitError> {
        let Some(mob) = entity.as_mob() else {
            return Err(SpawnInitError::ExpectedMob);
        };

        let overrides = &self.overrides;
        if let Some(attributes) = overrides.equine_attributes {
            attributes.apply(mob);
        }

        if overrides.is_baby {
            if let Some(ageable) = entity.as_ageable_mob() {
                ageable.set_age(ageable.get_baby_start_age());
            } else {
                mob.set_baby(true);
            }
        }

        self.mob.apply(mob);
        if let Some(can_pick_up_loot) = overrides.can_pick_up_loot {
            mob.set_can_pick_up_loot(can_pick_up_loot);
        }

        if let Some(item) = overrides.main_hand_item {
            mob.living_base()
                .equipment()
                .lock()
                .set(EquipmentSlot::MainHand, ItemStack::new(item));
        }

        if let Some(item) = overrides.head_item {
            mob.living_base()
                .equipment()
                .lock()
                .set(EquipmentSlot::Head, ItemStack::new(item));
            mob.set_equipment_drop_chance(EquipmentSlot::Head, 0.0);
        }

        if overrides.saddled {
            mob.living_base().equipment().lock().set(
                EquipmentSlot::Saddle,
                ItemStack::new(&vanilla_items::SADDLE),
            );
            mob.set_guaranteed_drop(EquipmentSlot::Saddle);
        }

        match self.initialize {
            Some(initialize) => initialize(entity),
            None => Ok(()),
        }
    }
}
