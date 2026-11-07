//! Vanilla Mooshroom (`MushroomCow`): mushroom stew, suspicious stew from fed
//! flowers, and shearing into a cow.

use crate::entity::prelude::*;
use std::sync::{Arc, LazyLock, Weak};

use glam::DVec3;
use rand::RngExt as _;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use simdnbt::{FromNbtTag as _, ToNbtTag as _};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{RandomizationType, SoundSource};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::components::SuspiciousStewEffects;
use steel_registry::data_components::vanilla_components::{
    MOOSHROOM_VARIANT, SUSPICIOUS_STEW_EFFECTS,
};
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::entity_variant::MooshroomVariant;
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::{ParticleData, SpellParticleOption};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::MooshroomEntityData;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_entities, vanilla_items, vanilla_loot_tables,
    vanilla_particle_types, vanilla_suspicious_stew_effects,
};
use steel_utils::color::RgbColor;
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, Downcast as _, DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::ageable::{Age, Ageable, AgeableProps};
use crate::entity::animal::{self, Breeding, BreedingProps};
use crate::entity::component::definition::MobDefinition;
use crate::entity::component::entries::MobInteractCtx;
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::entities::ItemEntity;
use crate::entity::entities::mobs::passive::cow::abstract_cow;
use crate::entity::shearable::{self, Shearable};
use crate::entity::{
    ConversionParams, DefaultEntityPush, EntityBase, EntityBaseLoad, EntityProps,
    EntitySpawnReason, LivingEntityBase, LivingProps, Mob, MobBase, MobProps, SharedEntity,
    SpawnRule, next_entity_id,
};
use crate::player::player_inventory::Prediction;
use crate::world::{LevelReader, World};

const MOOSHROOM_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.75, 0.0)];

/// `MushroomCow.BABY_DIMENSIONS`.
const MOOSHROOM_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    0.45,
    0.7,
    0.69,
    EntityAttachments::new(&MOOSHROOM_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);

/// `MushroomCow.MUTATE_CHANCE`: one in this many same-variant calves flips variant.
const MUTATE_CHANCE: u32 = 1024;

/// Vanilla mooshroom entity.
// TODO: `MushroomCow.thunderHit` swaps the variant once lightning bolts are ported.
#[entity_behavior(class = "MushroomCow", spawn_rule)]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_mooshroom, load = Self::load_mooshroom)]
pub struct MushroomCowEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    #[entity(component)]
    age: Age,
    #[entity(component)]
    breeding: Breeding,
    /// Effects of the last flower a brown mooshroom ate, for its next stew.
    stew_effects: CheckedMutex<Option<SuspiciousStewEffects>>,
    #[entity(synced)]
    entity_data: CheckedMutex<MooshroomEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `MushroomCowEntity`.
unsafe impl DowncastType for MushroomCowEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/mooshroom");
}

impl MushroomCowEntity {
    /// `MushroomCow.checkMushroomSpawnRules`: a mooshroom-spawnable block below
    /// and enough light, for every spawn reason.
    pub(crate) const SPAWN_RULE: SpawnRule =
        |_entity_type, level: &dyn LevelReader, _spawn_reason: EntitySpawnReason, pos, _random| {
            level
                .get_block_state(pos.below())
                .get_block()
                .has_tag(&BlockTag::MOOSHROOMS_SPAWNABLE_ON)
                && animal::is_bright_enough_to_spawn(level, pos)
        };

    const AGEABLE: AgeableProps = AgeableProps {
        breed_offspring: Self::breed_offspring,
    };
    const BREEDING: BreedingProps = BreedingProps::new(abstract_cow::is_food);
    const LIVING: LivingProps<Self> = LivingProps {
        default_dimensions: |mob, _| {
            if mob.living().is_baby() {
                MOOSHROOM_BABY_DIMENSIONS
            } else {
                mob.entity_type.dimensions
            }
        },
        ..abstract_cow::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        walk_target_value: Self::mob_walk_target_value,
        ..abstract_cow::mob_props()
    };
    const BOWL: StageId = StageId("mooshroom.bowl");
    const SHEARS: StageId = StageId("mooshroom.shears");
    const FLOWER: StageId = StageId("mooshroom.flower");

    /// Vanilla `MushroomCow`: the `AbstractCow` chains with stew, shearing and
    /// flower feeding ahead of the milk bucket.
    fn define() -> MobDefinition<Self> {
        let mut definition = abstract_cow::definition::<Self>();
        definition.mob_interact = definition.mob_interact.before(
            abstract_cow::MILK,
            [
                Stage::new(Self::BOWL, Self::fill_bowl),
                Stage::new(Self::SHEARS, Self::shear_by_player),
                Stage::new(Self::FLOWER, Self::eat_flower),
            ],
        );
        definition
    }

    fn definition() -> &'static MobDefinition<Self> {
        static DEFINITION: LazyLock<MobDefinition<MushroomCowEntity>> =
            LazyLock::new(MushroomCowEntity::define);
        &DEFINITION
    }

    /// Creates a new mooshroom at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a mooshroom from persisted base entity state.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = MooshroomEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(&Self::MOB, Self::definition()),
            age: Age::new(&Self::AGEABLE),
            breeding: Breeding::new(&Self::BREEDING),
            stew_effects: CheckedMutex::new(None),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// The mushroom variant.
    #[must_use]
    pub fn variant(&self) -> MooshroomVariant {
        MooshroomVariant::by_id(*self.entity_data.lock().mushroom_cow().variant_type.get())
    }

    /// Sets the mushroom variant.
    pub fn set_variant(&self, variant: MooshroomVariant) {
        self.entity_data
            .lock()
            .mushroom_cow_mut()
            .variant_type
            .set(variant.id());
    }

    /// The effects the next stew from this mooshroom carries.
    #[must_use]
    pub fn stew_effects(&self) -> Option<SuspiciousStewEffects> {
        self.stew_effects.lock().clone()
    }

    /// `MushroomCow.getWalkTargetValue`: mycelium first, then light.
    fn mob_walk_target_value(&self, pos: BlockPos) -> f32 {
        let Some(world) = self.level() else {
            return 0.0;
        };
        if world.get_block_state(pos.below()).get_block() == &vanilla_blocks::MYCELIUM {
            10.0
        } else {
            world.pathfinding_cost_from_light_levels(pos)
        }
    }

    /// `MushroomCow.mobInteract`: an adult fills a bowl with stew, suspicious
    /// if it ate a flower.
    fn fill_bowl(&self, ctx: &mut MobInteractCtx<'_>) -> Flow {
        let player = ctx.player;
        let holds_bowl = player
            .inventory
            .lock()
            .get_item_in_hand(ctx.hand)
            .is(&vanilla_items::BOWL);
        if !holds_bowl || self.animal().ageable().is_baby() {
            return Flow::Continue;
        }

        let stew_effects = self.stew_effects.lock().take();
        let (stew, sound) = match stew_effects {
            Some(effects) => {
                let mut stew = ItemStack::new(&vanilla_items::SUSPICIOUS_STEW);
                stew.set(SUSPICIOUS_STEW_EFFECTS, effects);
                (stew, &sound_events::ENTITY_MOOSHROOM_SUSPICIOUS_MILK)
            }
            None => (
                ItemStack::new(&vanilla_items::MUSHROOM_STEW),
                &sound_events::ENTITY_MOOSHROOM_MILK,
            ),
        };
        let has_infinite_materials = player.living().has_infinite_materials();
        let overflow = player.inventory.lock().apply_filled_result(
            ctx.hand,
            stew,
            has_infinite_materials,
            false,
        );
        if !overflow.is_empty() {
            let _ = player.drop_item(overflow, false, Prediction::Predicted);
        }
        self.play_sound(sound, 1.0, 1.0);
        ctx.result = InteractionResult::Success;
        Flow::Stop
    }

    /// `MushroomCow.mobInteract`: shears on an adult turn it into a cow.
    fn shear_by_player(&self, ctx: &mut MobInteractCtx<'_>) -> Flow {
        let holds_shears = ctx
            .player
            .inventory
            .lock()
            .get_item_in_hand(ctx.hand)
            .is(&vanilla_items::SHEARS);
        if !holds_shears || !self.ready_for_shearing() {
            return Flow::Continue;
        }
        shearable::shear_by_player(self, ctx.player, ctx.hand);
        ctx.result = InteractionResult::Success;
        Flow::Stop
    }

    /// `MushroomCow.mobInteract`: an adult brown mooshroom eats a flower whose
    /// effects its next stew carries, but only one at a time.
    fn eat_flower(&self, ctx: &mut MobInteractCtx<'_>) -> Flow {
        if self.variant() != MooshroomVariant::Brown || self.animal().ageable().is_baby() {
            return Flow::Continue;
        }
        let player = ctx.player;
        let held = player.inventory.lock().get_item_in_hand(ctx.hand).item();
        let Some(effects) = vanilla_suspicious_stew_effects::from_item(held) else {
            return Flow::Continue;
        };

        let center = DVec3::new(
            self.position().x,
            self.position().y + f64::from(self.base().dimensions().height) * 0.5,
            self.position().z,
        );
        let world = self.level();
        if self.stew_effects.lock().is_some() {
            if let Some(world) = world {
                world.send_particles_with_motion(
                    ParticleData::simple(&vanilla_particle_types::SMOKE),
                    center,
                    2,
                    DVec3::new(0.5, 0.0, 0.5),
                    DVec3::new(0.0, 0.2, 0.0),
                    RandomizationType::AlternativeWithSpeed,
                );
            }
        } else {
            let has_infinite_materials = player.living().has_infinite_materials();
            player
                .inventory
                .lock()
                .mutate_item_in_hand(ctx.hand, |item| item.consume_one(has_infinite_materials));
            if let Some(world) = world {
                world.send_particles_with_motion(
                    ParticleData::new(
                        &vanilla_particle_types::EFFECT,
                        SpellParticleOption::new(RgbColor::new(-1), 1.0),
                    ),
                    center,
                    4,
                    DVec3::new(0.5, 0.0, 0.5),
                    DVec3::new(0.0, 0.2, 0.0),
                    RandomizationType::AlternativeWithSpeed,
                );
            }
            *self.stew_effects.lock() = Some(effects);
            self.play_sound(&sound_events::ENTITY_MOOSHROOM_EAT, 2.0, 1.0);
        }
        ctx.result = InteractionResult::Success;
        Flow::Stop
    }

    /// `MushroomCow.getBreedOffspring`: either parent's variant, and rarely the
    /// other variant when both parents match.
    fn breed_offspring(
        parent: &Ageable<'_>,
        world: &Arc<World>,
        partner: &Ageable<'_>,
    ) -> Option<SharedEntity> {
        let offspring = parent.create_offspring(world)?;
        let variant_of =
            |ageable: &Ageable<'_>| ageable.entity().downcast_ref::<Self>().map(Self::variant);
        if let (Some(variant), Some(mate_variant), Some(calf)) = (
            variant_of(parent),
            variant_of(partner),
            offspring.downcast_ref::<Self>(),
        ) {
            let mut random = rand::rng();
            let calf_variant = if variant == mate_variant && random.random_ratio(1, MUTATE_CHANCE) {
                match variant {
                    MooshroomVariant::Red => MooshroomVariant::Brown,
                    MooshroomVariant::Brown => MooshroomVariant::Red,
                }
            } else if random.random::<bool>() {
                variant
            } else {
                mate_variant
            };
            calf.set_variant(calf_variant);
        }
        Some(offspring)
    }
}

impl Shearable for MushroomCowEntity {
    /// `MushroomCow.shear`: becomes a cow, dropping its mushrooms on top.
    fn shear(&self, world: &World, sound_source: SoundSource, tool: &ItemStack) {
        world.play_sound_at(
            &sound_events::ENTITY_MOOSHROOM_SHEAR,
            sound_source,
            self.position(),
            1.0,
            1.0,
            None,
        );
        let drop_mushrooms = |_cow: &Mob<'_>| {
            let Some(world) = self.level() else {
                return;
            };
            let position = self.position();
            let height = f64::from(self.base().dimensions().height);
            world.send_particles(
                ParticleData::simple(&vanilla_particle_types::EXPLOSION),
                position + DVec3::Y * (height * 0.5),
                1,
                DVec3::ZERO,
                0.0,
            );
            for drop in self
                .living()
                .shearing_loot(&vanilla_loot_tables::SHEARING_MOOSHROOM, tool)
            {
                for _ in 0..drop.count() {
                    let item = ItemEntity::with_item(
                        &vanilla_entities::ITEM,
                        next_entity_id(),
                        position + DVec3::Y * height,
                        drop.copy_with_count(1),
                        Arc::downgrade(&world),
                    );
                    let _ = world.try_add_entity(Arc::new(item));
                }
            }
        };
        let _ = self.mob().convert_to(
            &vanilla_entities::COW,
            ConversionParams::single(false, false),
            Some(&drop_mushrooms),
        );
    }

    fn ready_for_shearing(&self) -> bool {
        !self.animal().ageable().is_baby()
    }
}

impl DefaultEntityPush for MushroomCowEntity {}

impl MushroomCowEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        apply_implicit_item_components: |mooshroom, item_stack| {
            if let Some(variant) = item_stack.get(MOOSHROOM_VARIANT) {
                mooshroom.set_variant(*variant);
            }
        },
        ..abstract_cow::entity_props()
    };

    fn save_mooshroom(&self, nbt: &mut NbtCompound) {
        nbt.insert("Type", self.variant().serialized_name());
        if let Some(stew_effects) = self.stew_effects.lock().as_ref() {
            nbt.insert("stew_effects", stew_effects.to_nbt_tag());
        }
    }

    fn load_mooshroom(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let variant = nbt
            .string("Type")
            .and_then(|name| MooshroomVariant::from_serialized_name(name.to_str().as_ref()))
            .unwrap_or_default();
        self.set_variant(variant);
        *self.stew_effects.lock() = nbt
            .get("stew_effects")
            .and_then(SuspiciousStewEffects::from_nbt_tag);
    }
}

#[cfg(test)]
mod tests;
