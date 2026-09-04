//! Runtime sculk spreading system for Sculk Catalyst blocks.
//!
//! Vanilla `SculkSpreader` that persists in catalyst block entity NBT and spreads
//! sculk when mobs die nearby. This is the runtime counterpart to the worldgen
//! spreading system in `worldgen/feature/features/sculk_patch.rs`.

use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::{TaggedRegistryExt, REGISTRY};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::world::World;
use std::sync::Arc;

const MAX_CHARGE: i32 = 1000;
const MAX_CURSORS: usize = 32;

/// Runtime sculk spreading state for a Sculk Catalyst.
#[derive(Default)]
pub struct SculkSpreader {
    /// Active charge cursors spreading sculk
    cursors: Vec<SculkChargeCursor>,
}

/// A cursor of sculk charge that moves through the world converting blocks
#[derive(Clone, Debug)]
struct SculkChargeCursor {
    pos: BlockPos,
    charge: i32,
    update_delay: i32,
    decay_delay: i32,
    facings: Option<Vec<Direction>>,
}

impl SculkChargeCursor {
    fn new(pos: BlockPos, charge: i32) -> Self {
        Self {
            pos,
            charge,
            update_delay: 0,
            decay_delay: 1,
            facings: None,
        }
    }

    fn save(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("charge", self.charge);
        nbt.insert("decay_delay", self.decay_delay);
        nbt.insert("update_delay", self.update_delay);
        let mut pos_nbt = NbtCompound::new();
        pos_nbt.insert("X", self.pos.x());
        pos_nbt.insert("Y", self.pos.y());
        pos_nbt.insert("Z", self.pos.z());
        nbt.insert("pos", pos_nbt);
        if let Some(ref facings) = self.facings {
            let facing_indices: Vec<i32> = facings.iter().map(|d| d.get_3d_data_value()).collect();
            nbt.insert("facings", NbtTag::IntArray(facing_indices));
        }
        nbt
    }

    fn load(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
        let charge = nbt.int("charge")?;
        let decay_delay = nbt.int("decay_delay").unwrap_or(1);
        let update_delay = nbt.int("update_delay").unwrap_or(0);
        let pos_nbt = nbt.compound("pos")?;
        let pos = BlockPos::new(
            pos_nbt.int("X")?,
            pos_nbt.int("Y")?,
            pos_nbt.int("Z")?,
        );
        let facings = nbt.int_array("facings").map(|arr| {
            arr.iter()
                .filter_map(|&idx| Some(Direction::from_3d_data_value(idx)))
                .collect()
        });
        Some(Self {
            pos,
            charge,
            update_delay,
            decay_delay,
            facings,
        })
    }
}

impl SculkSpreader {
    /// Creates a new runtime spreader
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds charge to spread from a death position
    pub fn add_cursors_from_death(&mut self, death_pos: BlockPos, mut charge: i32) {
        while charge > 0 && self.cursors.len() < MAX_CURSORS {
            let current_charge = charge.min(MAX_CHARGE);
            self.cursors.push(SculkChargeCursor::new(death_pos, current_charge));
            charge -= current_charge;
        }
    }

    /// Ticks all active cursors, spreading sculk
    pub fn tick(&mut self, world: &Arc<World>, catalyst_pos: BlockPos) -> bool {
        if self.cursors.is_empty() {
            return false;
        }

        let mut changed = false;
        let cursors = std::mem::take(&mut self.cursors);

        for mut cursor in cursors {
            if cursor.update_delay > 0 {
                cursor.update_delay -= 1;
                self.cursors.push(cursor);
                continue;
            }

            let charge_before = cursor.charge;
            self.update_cursor(world, catalyst_pos, &mut cursor);

            if cursor.charge != charge_before {
                changed = true;
            }

            if cursor.charge > 0 {
                self.cursors.push(cursor);
            }
        }

        changed
    }

    fn update_cursor(&mut self, world: &Arc<World>, catalyst_pos: BlockPos, cursor: &mut SculkChargeCursor) {
        if cursor.charge <= 0 {
            return;
        }

        let current_state = world.get_block_state(cursor.pos);

        // Try to consume charge for spreading
        cursor.charge = self.attempt_use_charge(world, catalyst_pos, cursor, current_state);

        if cursor.charge <= 0 {
            return;
        }

        // Try to move to adjacent position
        if let Some(new_pos) = self.find_movement_pos(world, cursor.pos) {
            cursor.pos = new_pos;
            cursor.update_delay = 1;
        }

        cursor.decay_delay = 1;
    }

    fn attempt_use_charge(
        &self,
        world: &Arc<World>,
        catalyst_pos: BlockPos,
        cursor: &SculkChargeCursor,
        state: BlockStateId,
    ) -> i32 {
        let block = state.get_block();

        // Already sculk - decay slower
        if block == &vanilla_blocks::SCULK {
            if cursor.decay_delay > 0 {
                return cursor.charge;
            }
            // Random decay
            if (world.game_time() + i64::from(cursor.pos.x())) % 5 != 0 {
                return cursor.charge;
            }
            return cursor.charge - 1;
        }

        // Not replaceable - decay
        if !REGISTRY.blocks.is_in_tag(block, &BlockTag::SCULK_REPLACEABLE) {
            if cursor.decay_delay > 0 {
                return cursor.charge;
            }
            return (cursor.charge / 2).max(0);
        }

        // Can place sculk here
        let dist_from_catalyst = manhattan_distance(cursor.pos, catalyst_pos);
        if dist_from_catalyst > 1 {
            // Try to convert this block
            let cost = 5; // Cost per conversion
            if cursor.charge >= cost {
                world.set_block(cursor.pos, vanilla_blocks::SCULK.default_state(), steel_utils::types::UpdateFlags::UPDATE_ALL);
                return cursor.charge - cost;
            }
        }

        cursor.charge
    }

    fn find_movement_pos(&self, world: &Arc<World>, pos: BlockPos) -> Option<BlockPos> {
        // Try all 6 adjacent positions
        for direction in Direction::ALL {
            let neighbor = pos.relative(direction);
            let state = world.get_block_state(neighbor);

            // Prefer sculk or replaceable blocks
            if state.get_block() == &vanilla_blocks::SCULK
                || REGISTRY.blocks.is_in_tag(state.get_block(), &BlockTag::SCULK_REPLACEABLE)
            {
                return Some(neighbor);
            }
        }
        None
    }

    /// Saves spreader state to NBT
    pub fn save(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        let cursor_list: Vec<NbtCompound> = self.cursors.iter().map(|c| c.save()).collect();
        nbt.insert("cursors", NbtTag::List(NbtList::Compound(cursor_list)));
        nbt
    }

    /// Loads spreader state from NBT
    pub fn load(nbt: &NbtCompoundView<'_, '_>) -> Self {
        let cursors = if let Some(list) = nbt.list("cursors") {
            if let Some(compounds) = list.compounds() {
                compounds
                    .into_iter()
                    .filter_map(|c| SculkChargeCursor::load(&c.into()))
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Self { cursors }
    }

    /// Returns whether there are active cursors
    pub fn has_active_cursors(&self) -> bool {
        !self.cursors.is_empty()
    }
}

fn manhattan_distance(a: BlockPos, b: BlockPos) -> i32 {
    (a.x() - b.x()).abs() + (a.y() - b.y()).abs() + (a.z() - b.z()).abs()
}
