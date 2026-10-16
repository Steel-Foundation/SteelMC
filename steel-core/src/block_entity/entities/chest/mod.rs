use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::{borrow::BaseNbtCompound, owned::NbtCompound};
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    blocks::{
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, ChestType},
    },
    data_components::vanilla_components::SeededContainerLoot,
    item_stack::ItemStack,
    vanilla_block_entity_types, vanilla_game_events,
};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, translations};
use text_components::TextComponent;

use crate::{
    behavior::{BLOCK_BEHAVIORS, blocks::ChestBlock},
    block_entity::{
        BlockEntity, BlockEntityBase, BlockEntityLifecycleExt as _, RandomizableContainer,
        container_openers_counter::{ContainerOpenersCounter, OpenersChange},
        container_properties::ContainerProperties,
        randomizable_container::RandomizableContainerStorage,
    },
    entity::{ContainerUser, Entity},
    inventory::lock::ContainerRef,
    loot::LootResult,
    nameable::Nameable,
    player::Player,
    world::{World, game_event::GameEventContext},
};

/// Slots stored independently by each half of a chest.
pub const CHEST_SLOTS: usize = 27;

/// Normal chest inventory, loot, name/lock, and open-count state.
pub struct ChestBlockEntity {
    storage: RandomizableContainerStorage,
    properties: ContainerProperties,
    openers: ContainerOpenersCounter,
}

// SAFETY: This Steel-owned key uniquely identifies `ChestBlockEntity`.
unsafe impl DowncastType for ChestBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/chest");
}

impl ChestBlockEntity {
    /// Creates one independently persisted chest half.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            storage: RandomizableContainerStorage::new(
                &vanilla_block_entity_types::CHEST,
                CHEST_SLOTS,
                level,
                pos,
                state,
            ),
            properties: ContainerProperties::default(),
            openers: ContainerOpenersCounter::default(),
        }
    }

    /// Checks the lock and the spectator restriction on unopened loot.
    pub(crate) fn can_open(&self, player: &Player) -> bool {
        (!player.is_spectator() || self.loot_table().is_none()) && self.properties.can_open(player)
    }

    /// Reconciles the counter with live container users after a scheduled tick.
    pub fn recheck_open(&self) {
        if self.is_removed() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };
        let change = self.openers.recheck(
            &world,
            self.get_block_pos(),
            self.storage.container_ref.container_id(),
        );
        let open = change.current > 0;
        self.publish_openers_change(&world, change, None);
        if open {
            self.schedule_recheck(&world);
        }
    }

    fn schedule_recheck(&self, world: &World) {
        world.schedule_block_tick_default(
            self.get_block_pos(),
            self.get_block_state().get_block(),
            5,
        );
    }

    fn publish_openers_change(
        &self,
        world: &Arc<World>,
        change: OpenersChange,
        source: Option<&dyn Entity>,
    ) {
        let state = self.get_block_state();
        let pos = self.get_block_pos();
        let opening = change.previous == 0 && change.current != 0;
        let closing = change.previous != 0 && change.current == 0;
        if opening || closing {
            self.play_sound(world, state, opening);
            world.game_event(
                if opening {
                    &vanilla_game_events::CONTAINER_OPEN
                } else {
                    &vanilla_game_events::CONTAINER_CLOSE
                },
                pos,
                &GameEventContext::new(source, None),
            );
        }
        world.block_event(pos, state.get_block(), 1, change.current);
    }

    fn play_sound(&self, world: &World, state: BlockStateId, opening: bool) {
        let chest_type = state.get_value(&BlockStateProperties::CHEST_TYPE);
        if chest_type == ChestType::Left {
            return;
        }
        let Some((open_sound, close_sound)) = BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .chest_sounds()
        else {
            return;
        };
        let pos = self.get_block_pos();
        let mut center = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        if chest_type == ChestType::Right {
            center += DVec3::from(ChestBlock::connected_direction(state).offset_vec()) * 0.5;
        }
        world.play_sound_at(
            if opening { open_sound } else { close_sound },
            SoundSource::Blocks,
            center,
            0.5,
            rand::random_range(0.9..1.0),
            None,
        );
    }
}

impl BlockEntity for ChestBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.storage.base
    }
    fn load_additional(&self, nbt: &BaseNbtCompound<'_>) {
        self.properties.load(nbt);
        self.storage.load_additional(nbt);
    }
    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.properties.save(nbt);
        self.storage.save_additional(nbt);
    }
    fn apply_components_from_item(&self, item: &ItemStack) {
        self.properties.apply_components(item);
        self.storage.apply_components_from_item(item);
    }
    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        self.storage.pre_remove_side_effects(pos);
    }
    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.storage.container_ref.clone())
    }
    fn as_nameable(&self) -> Option<&dyn Nameable> {
        Some(self)
    }
    fn as_randomizable_container(&self) -> Option<&dyn RandomizableContainer> {
        Some(self)
    }
    fn start_open(&self, user: &dyn ContainerUser) {
        if self.is_removed() || user.is_spectator() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };
        let change = self.openers.increment(user);
        let first = change.previous == 0;
        self.publish_openers_change(&world, change, Some(user));
        if first {
            self.schedule_recheck(&world);
        }
    }
    fn stop_open(&self, user: &dyn ContainerUser) {
        if self.is_removed() || user.is_spectator() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };
        self.publish_openers_change(&world, self.openers.decrement(), Some(user));
    }
    fn trigger_event(&self, param_a: i32, _param_b: i32) -> bool {
        // Lid interpolation is client-side; the server publishes the open-count event.
        param_a == 1
    }
}

impl RandomizableContainer for ChestBlockEntity {
    fn loot_table(&self) -> Option<SeededContainerLoot> {
        self.storage.loot_table()
    }
    fn set_loot_table(&self, loot: Option<SeededContainerLoot>) {
        self.storage.set_loot_table(loot);
    }
    fn unpack_loot_table(&self, player: Option<&Player>) -> LootResult<()> {
        self.storage.unpack_loot_table(player)
    }
}

impl Nameable for ChestBlockEntity {
    fn custom_name(&self) -> Option<TextComponent> {
        self.properties.custom_name()
    }
    fn name(&self) -> TextComponent {
        self.properties
            .custom_name()
            .unwrap_or_else(|| translations::CONTAINER_CHEST.msg().component())
    }
}
