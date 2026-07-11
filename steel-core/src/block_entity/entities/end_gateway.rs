//! End gateway block entity.

use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{vanilla_block_entity_types, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityTickAction};
use crate::world::World;

const SPAWN_TIME: i64 = 200;
const COOLDOWN_TIME: i32 = 40;
const ATTENTION_INTERVAL: i64 = 2400;
const EVENT_COOLDOWN: u8 = 1;

/// Vanilla `TheEndGatewayBlockEntity`.
pub struct EndGatewayBlockEntity {
    world: Weak<World>,
    pos: BlockPos,
    state: BlockStateId,
    removed: bool,
    age: i64,
    teleport_cooldown: i32,
    exit_portal: Option<BlockPos>,
    exact_teleport: bool,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EndGatewayBlockEntity`.
unsafe impl DowncastType for EndGatewayBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/end_gateway");
}

impl EndGatewayBlockEntity {
    /// Creates an End gateway block entity with vanilla default state.
    #[must_use]
    pub const fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            world,
            pos,
            state,
            removed: false,
            age: 0,
            teleport_cooldown: 0,
            exit_portal: None,
            exact_teleport: false,
        }
    }

    /// Returns vanilla `TheEndGatewayBlockEntity.isSpawning`.
    #[must_use]
    pub const fn is_spawning(&self) -> bool {
        self.age < SPAWN_TIME
    }

    /// Returns vanilla `TheEndGatewayBlockEntity.isCoolingDown`.
    #[must_use]
    pub const fn is_cooling_down(&self) -> bool {
        self.teleport_cooldown > 0
    }

    /// Returns the stored gateway exit position.
    #[must_use]
    pub const fn exit_portal(&self) -> Option<BlockPos> {
        self.exit_portal
    }

    /// Returns whether the stored exit is used exactly.
    #[must_use]
    pub const fn exact_teleport(&self) -> bool {
        self.exact_teleport
    }

    /// Sets the stored gateway exit position.
    pub fn set_exit_position(&mut self, exact_position: BlockPos, exact: bool) {
        self.exact_teleport = exact;
        self.exit_portal = Some(exact_position);
        self.set_changed();
    }

    /// Triggers vanilla gateway cooldown and broadcasts the block event.
    pub fn trigger_cooldown(&mut self, world: &World) {
        self.teleport_cooldown = COOLDOWN_TIME;
        world.block_event(self.pos, self.state.get_block(), EVENT_COOLDOWN, 0);
        self.set_changed();
    }

    const fn nbt_bool(value: bool) -> i8 {
        value as i8
    }

    fn load_exit_portal(nbt: &NbtCompoundView<'_, '_>) -> Option<BlockPos> {
        let exit = nbt.int_array("exit_portal")?;
        if exit.len() != 3 {
            return None;
        }
        let pos = BlockPos::new(exit[0], exit[1], exit[2]);
        World::is_in_spawnable_bounds(pos).then_some(pos)
    }
}

impl BlockEntity for EndGatewayBlockEntity {
    fn get_type(&self) -> BlockEntityTypeRef {
        &vanilla_block_entity_types::END_GATEWAY
    }

    fn get_block_pos(&self) -> BlockPos {
        self.pos
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.world.upgrade()
    }

    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        self.age = nbt.long("Age").unwrap_or(0);
        self.exit_portal = Self::load_exit_portal(&nbt);
        self.exact_teleport = nbt.byte("ExactTeleport").is_some_and(|value| value != 0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("Age", self.age);
        if let Some(exit) = self.exit_portal {
            nbt.insert(
                "exit_portal",
                NbtTag::IntArray(vec![exit.x(), exit.y(), exit.z()]),
            );
        }
        if self.exact_teleport {
            nbt.insert("ExactTeleport", Self::nbt_bool(true));
        }
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        self.save_additional(&mut nbt);
        Some(nbt)
    }

    fn is_ticking(&self) -> bool {
        true
    }

    fn tick(&mut self, world: &Arc<World>) -> Option<BlockEntityTickAction> {
        let state = world.get_block_state(self.pos);
        if state.get_block() != &vanilla_blocks::END_GATEWAY {
            self.set_removed();
            return None;
        }

        self.state = state;
        let was_spawning = self.is_spawning();
        let was_cooling_down = self.is_cooling_down();

        self.age += 1;
        if was_cooling_down {
            self.teleport_cooldown -= 1;
        } else if self.age % ATTENTION_INTERVAL == 0 {
            self.trigger_cooldown(world);
        }

        if was_spawning != self.is_spawning() || was_cooling_down != self.is_cooling_down() {
            self.set_changed();
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Weak;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    fn load_from_owned_nbt(gateway: &mut EndGatewayBlockEntity, nbt: &NbtCompound) {
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test nbt should reborrow");
        gateway.load_additional(&borrowed);
    }

    fn gateway() -> EndGatewayBlockEntity {
        init_test_registry();
        EndGatewayBlockEntity::new(
            Weak::new(),
            BlockPos::new(4, 65, -9),
            vanilla_blocks::END_GATEWAY.default_state(),
        )
    }

    #[test]
    fn end_gateway_saves_vanilla_nbt_keys() {
        let mut gateway = gateway();
        gateway.age = 12;
        gateway.set_exit_position(BlockPos::new(100, 72, -32), true);

        let mut nbt = NbtCompound::new();
        gateway.save_additional(&mut nbt);

        assert_eq!(nbt.long("Age"), Some(12));
        assert_eq!(
            nbt.int_array("exit_portal").map(<[i32]>::to_vec),
            Some(vec![100, 72, -32])
        );
        assert_eq!(nbt.byte("ExactTeleport"), Some(1));
    }

    #[test]
    fn full_metadata_includes_type_and_position_after_additional_data() {
        let gateway = gateway();
        let nbt = gateway.save_with_full_metadata();

        assert_eq!(
            nbt.string("id").map(ToString::to_string),
            Some("minecraft:end_gateway".to_owned())
        );
        assert_eq!(nbt.int("x"), Some(4));
        assert_eq!(nbt.int("y"), Some(65));
        assert_eq!(nbt.int("z"), Some(-9));
        assert_eq!(nbt.long("Age"), Some(0));
    }

    #[test]
    fn end_gateway_loads_vanilla_nbt_keys() {
        let mut nbt = NbtCompound::new();
        nbt.insert("Age", 44_i64);
        nbt.insert("exit_portal", NbtTag::IntArray(vec![8, 70, 12]));
        nbt.insert("ExactTeleport", 1_i8);

        let mut gateway = gateway();
        load_from_owned_nbt(&mut gateway, &nbt);

        assert_eq!(gateway.age, 44);
        assert_eq!(gateway.exit_portal(), Some(BlockPos::new(8, 70, 12)));
        assert!(gateway.exact_teleport());
    }

    #[test]
    fn end_gateway_rejects_exit_outside_spawnable_bounds() {
        let mut nbt = NbtCompound::new();
        nbt.insert("exit_portal", NbtTag::IntArray(vec![0, 20_000_000, 0]));

        let mut gateway = gateway();
        load_from_owned_nbt(&mut gateway, &nbt);

        assert_eq!(gateway.exit_portal(), None);
    }
}
