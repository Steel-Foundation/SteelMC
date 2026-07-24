//! End gateway block entity.

use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{vanilla_block_entity_types, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase, BlockEntityLifecycleExt as _};
use crate::world::World;

const SPAWN_TIME: i64 = 200;
const COOLDOWN_TIME: i32 = 40;
const ATTENTION_INTERVAL: i64 = 2400;
const EVENT_COOLDOWN: i32 = 1;

/// Vanilla `TheEndGatewayBlockEntity`.
pub struct EndGatewayBlockEntity {
    base: BlockEntityBase,
    gateway: SyncMutex<EndGatewayState>,
}

#[derive(Clone, Copy)]
struct EndGatewayState {
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
    pub fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::END_GATEWAY, world, pos, state),
            gateway: SyncMutex::new(EndGatewayState {
                age: 0,
                teleport_cooldown: 0,
                exit_portal: None,
                exact_teleport: false,
            }),
        }
    }

    /// Returns vanilla `TheEndGatewayBlockEntity.isSpawning`.
    #[must_use]
    pub fn is_spawning(&self) -> bool {
        self.gateway.lock().age < SPAWN_TIME
    }

    /// Returns vanilla `TheEndGatewayBlockEntity.isCoolingDown`.
    #[must_use]
    pub fn is_cooling_down(&self) -> bool {
        self.gateway.lock().teleport_cooldown > 0
    }

    /// Returns the stored gateway exit position.
    #[must_use]
    pub fn exit_portal(&self) -> Option<BlockPos> {
        self.gateway.lock().exit_portal
    }

    /// Returns whether the stored exit is used exactly.
    #[must_use]
    pub fn exact_teleport(&self) -> bool {
        self.gateway.lock().exact_teleport
    }

    /// Sets the stored gateway exit position.
    pub fn set_exit_position(&self, exact_position: BlockPos, exact: bool) {
        {
            let mut gateway = self.gateway.lock();
            gateway.exact_teleport = exact;
            gateway.exit_portal = Some(exact_position);
        }
        self.set_changed();
    }

    /// Triggers vanilla gateway cooldown and broadcasts the block event.
    pub fn trigger_cooldown(&self, world: &World) {
        self.gateway.lock().teleport_cooldown = COOLDOWN_TIME;
        world.block_event(
            self.get_block_pos(),
            self.get_block_state().get_block(),
            EVENT_COOLDOWN,
            0,
        );
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
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn trigger_event(&self, param_a: i32, _param_b: i32) -> bool {
        if param_a != EVENT_COOLDOWN {
            return false;
        }
        self.gateway.lock().teleport_cooldown = COOLDOWN_TIME;
        true
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let mut gateway = self.gateway.lock();
        gateway.age = nbt.long("Age").unwrap_or(0);
        gateway.exit_portal = Self::load_exit_portal(&nbt);
        gateway.exact_teleport = nbt.byte("ExactTeleport").is_some_and(|value| value != 0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let gateway = self.gateway.lock();
        nbt.insert("Age", gateway.age);
        if let Some(exit) = gateway.exit_portal {
            nbt.insert(
                "exit_portal",
                NbtTag::IntArray(vec![exit.x(), exit.y(), exit.z()]),
            );
        }
        if gateway.exact_teleport {
            nbt.insert("ExactTeleport", Self::nbt_bool(true));
        }
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        self.save_additional(&mut nbt);
        Some(nbt)
    }

    fn tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let state = world.get_block_state(pos);
        if state.get_block() != &vanilla_blocks::END_GATEWAY {
            self.set_removed();
            return;
        }

        self.set_block_state(state);
        let (trigger_cooldown, lifecycle_changed) = {
            let mut gateway = self.gateway.lock();
            let was_spawning = gateway.age < SPAWN_TIME;
            let was_cooling_down = gateway.teleport_cooldown > 0;
            gateway.age += 1;
            let trigger_cooldown = if was_cooling_down {
                gateway.teleport_cooldown -= 1;
                false
            } else if gateway.age % ATTENTION_INTERVAL == 0 {
                gateway.teleport_cooldown = COOLDOWN_TIME;
                true
            } else {
                false
            };
            let lifecycle_changed = was_spawning != (gateway.age < SPAWN_TIME)
                || was_cooling_down != (gateway.teleport_cooldown > 0);
            (trigger_cooldown, lifecycle_changed)
        };

        if trigger_cooldown {
            world.block_event(pos, state.get_block(), EVENT_COOLDOWN, 0);
            self.set_changed();
        }
        if lifecycle_changed {
            self.set_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Weak;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    fn load_from_owned_nbt(gateway: &EndGatewayBlockEntity, nbt: &NbtCompound) {
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
        let gateway = gateway();
        gateway.gateway.lock().age = 12;
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

        let gateway = gateway();
        load_from_owned_nbt(&gateway, &nbt);

        assert_eq!(gateway.gateway.lock().age, 44);
        assert_eq!(gateway.exit_portal(), Some(BlockPos::new(8, 70, 12)));
        assert!(gateway.exact_teleport());
    }

    #[test]
    fn end_gateway_rejects_exit_outside_spawnable_bounds() {
        let mut nbt = NbtCompound::new();
        nbt.insert("exit_portal", NbtTag::IntArray(vec![0, 20_000_000, 0]));

        let gateway = gateway();
        load_from_owned_nbt(&gateway, &nbt);

        assert_eq!(gateway.exit_portal(), None);
    }
}
