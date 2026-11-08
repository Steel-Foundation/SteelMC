//! Vanilla `Interaction`: an invisible box that records player clicks.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::vanilla_entity_data::InteractionEntityData;
use steel_utils::UuidExt;
use steel_utils::locks::{CheckedMutex, CheckedMutexGuard};
use uuid::Uuid;

use crate::behavior::InteractionResult;
use crate::entity::component::definition::Definition;
use crate::entity::prelude::*;
use crate::entity::{EntityBase, EntityBaseLoad, EntityProps, SharedEntity};
use crate::player::Player;
use crate::world::World;

const DEFAULT_WIDTH: f32 = 1.0;
const DEFAULT_HEIGHT: f32 = 1.0;
const DEFAULT_DIMENSIONS: EntityDimensions =
    EntityDimensions::with_default_eye_height(DEFAULT_WIDTH, DEFAULT_HEIGHT);

/// An invisible, invincible box whose size is configurable and which records
/// the last player to attack it and the last to interact with it.
#[entity_behavior(class = "Interaction")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_interaction, load = Self::load_interaction)]
#[entity(key = "steel:entity/interaction", definition = || Definition::replacing_tick([]))]
pub struct InteractionEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    actions: CheckedMutex<RecordedActions>,
    #[entity(synced)]
    entity_data: CheckedMutex<InteractionEntityData>,
}

#[derive(Default)]
struct RecordedActions {
    attack: Option<PlayerAction>,
    interaction: Option<PlayerAction>,
}

impl InteractionEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        hurt: |_, _, _, _| false,
        is_pickable: |_| true,
        can_be_hit_by_projectile: |_| false,
        piston_push_reaction: |_| PushReaction::IgnoreEntity,
        is_ignoring_block_triggers: |_| true,
        skip_attack_interaction: Self::entity_skip_attack_interaction,
        interact: |interaction, player, _, _| interaction.record_interaction(player),
        dimensions_for_pose: |interaction, _| {
            let data = interaction.entity_data.lock();
            EntityDimensions::with_default_eye_height(*data.width.get(), *data.height.get())
        },
        ..EntityProps::new()
    };

    /// Creates a new interaction.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, DEFAULT_DIMENSIONS, world),
            entity_type,
        )
    }

    /// Reconstructs an interaction from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(EntityBase::from_load(load, DEFAULT_DIMENSIONS), entity_type)
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        base.set_no_physics(true);
        Self {
            base,
            entity_type,
            actions: CheckedMutex::new(RecordedActions::default()),
            entity_data: CheckedMutex::new(InteractionEntityData::new()),
        }
    }

    /// Edits the box size and `response` flag; a size change refreshes the
    /// dimensions. Do not lock the entity data inside `f`.
    pub fn with_entity_data<R>(
        &self,
        f: impl FnOnce(&mut InteractionEntityDataView<'_>) -> R,
    ) -> R {
        let (value, dimensions_changed) = {
            let mut view = InteractionEntityDataView {
                guard: self.entity_data.lock(),
                dimensions_changed: false,
            };
            let value = f(&mut view);
            (value, view.dimensions_changed)
        };
        if dimensions_changed {
            self.refresh_dimensions();
        }
        value
    }

    /// The last attack (left-click), if any.
    #[must_use]
    pub fn last_attack(&self) -> Option<PlayerAction> {
        self.actions.lock().attack
    }

    /// The last interaction (right-click), if any.
    #[must_use]
    pub fn last_interaction(&self) -> Option<PlayerAction> {
        self.actions.lock().interaction
    }

    /// `Interaction.skipAttackInteraction`: records a player's attack, and lets
    /// the attack through only when `response` is set.
    fn entity_skip_attack_interaction(&self, source: &SharedEntity) -> bool {
        let Some(player) = source.as_player() else {
            return false;
        };
        if let Some(action) = self.action_by(player) {
            self.actions.lock().attack = Some(action);
            // TODO: trigger PLAYER_HURT_ENTITY once advancement criteria exist.
        }
        !*self.entity_data.lock().response.get()
    }

    /// `Interaction.interact`: records a player's interaction.
    fn record_interaction(&self, player: &Player) -> InteractionResult {
        if let Some(action) = self.action_by(player) {
            self.actions.lock().interaction = Some(action);
        }
        InteractionResult::Consume
    }

    fn action_by(&self, player: &Player) -> Option<PlayerAction> {
        let world = self.level()?;
        Some(PlayerAction {
            player: player.uuid(),
            timestamp: world.game_time(),
        })
    }

    fn save_interaction(&self, nbt: &mut NbtCompound) {
        {
            let data = self.entity_data.lock();
            nbt.insert("width", *data.width.get());
            nbt.insert("height", *data.height.get());
            nbt.insert("response", *data.response.get());
        }
        let actions = self.actions.lock();
        if let Some(attack) = actions.attack.as_ref() {
            nbt.insert("attack", attack);
        }
        if let Some(interaction) = actions.interaction.as_ref() {
            nbt.insert("interaction", interaction);
        }
    }

    fn load_interaction(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.with_entity_data(|data| {
            data.set_width(nbt.float("width").unwrap_or(DEFAULT_WIDTH));
            data.set_height(nbt.float("height").unwrap_or(DEFAULT_HEIGHT));
            data.set_response(nbt.byte("response").is_some_and(|value| value != 0));
        });
        let mut actions = self.actions.lock();
        actions.attack = nbt.get("attack").and_then(PlayerAction::from_nbt_tag);
        actions.interaction = nbt.get("interaction").and_then(PlayerAction::from_nbt_tag);
    }
}

/// A player's click: who, and the game time it happened.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PlayerAction {
    player: Uuid,
    timestamp: i64,
}

impl PlayerAction {
    /// The player who clicked.
    #[must_use]
    pub const fn player(&self) -> Uuid {
        self.player
    }

    /// The game time of the click, in ticks.
    #[must_use]
    pub const fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

impl FromNbtTag for PlayerAction {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            player: Uuid::from_int_array(&compound.int_array("player")?)?,
            timestamp: compound.long("timestamp")?,
        })
    }
}

impl ToNbtTag for &PlayerAction {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert(
            "player",
            NbtTag::IntArray(self.player.to_int_array().to_vec()),
        );
        compound.insert("timestamp", self.timestamp);
        NbtTag::Compound(compound)
    }
}

/// Exclusive access to an interaction's box size and `response` flag.
pub struct InteractionEntityDataView<'a> {
    guard: CheckedMutexGuard<'a, InteractionEntityData>,
    dimensions_changed: bool,
}

impl InteractionEntityDataView<'_> {
    /// The box width.
    #[must_use]
    pub fn width(&self) -> f32 {
        *self.guard.width.get()
    }

    /// Sets the box width.
    pub fn set_width(&mut self, width: f32) {
        self.guard.width.set(width);
        self.dimensions_changed |= self.guard.width.is_dirty();
    }

    /// The box height.
    #[must_use]
    pub fn height(&self) -> f32 {
        *self.guard.height.get()
    }

    /// Sets the box height.
    pub fn set_height(&mut self, height: f32) {
        self.guard.height.set(height);
        self.dimensions_changed |= self.guard.height.is_dirty();
    }

    /// Whether clicks get a player response: a punch sound for attacks and a
    /// hand swing for interactions.
    #[must_use]
    pub fn response(&self) -> bool {
        *self.guard.response.get()
    }

    /// Sets whether clicks get a player response.
    pub fn set_response(&mut self, response: bool) {
        self.guard.response.set(response);
    }
}

#[cfg(test)]
mod tests;
