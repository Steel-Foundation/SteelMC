//! Block breaking state machine for players.
//!
//! This module implements the logic from Java's `ServerPlayerGameMode` for handling
//! block breaking, including progress tracking and validation.

use steel_protocol::packets::game::CBlockUpdate;
use steel_registry::{REGISTRY, blocks::properties::Direction, vanilla_blocks};
use steel_utils::{
    BlockPos, BlockStateId,
    types::{GameType, InteractionHand, UpdateFlags},
};

use crate::player::Player;
use crate::world::World;

/// Manages the block breaking state for a player.
///
/// Based on Java's `ServerPlayerGameMode` fields and logic.
pub struct BlockBreakingManager {
    /// Whether the player is currently breaking a block.
    is_destroying_block: bool,
    /// The tick when destruction started.
    destroy_progress_start: u64,
    /// The position of the block being destroyed.
    destroy_pos: BlockPos,
    /// The current game tick counter.
    game_ticks: u64,
    /// Whether there's a delayed destroy pending (for slow mining).
    has_delayed_destroy: bool,
    /// Position of the delayed destroy.
    delayed_destroy_pos: BlockPos,
    /// The tick when delayed destroy started.
    delayed_tick_start: u64,
    /// The last sent destruction progress state (0-9, or -1 for none).
    last_sent_state: i32,
}

impl Default for BlockBreakingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockBreakingManager {
    /// Creates a new block breaking manager.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            is_destroying_block: false,
            destroy_progress_start: 0,
            destroy_pos: BlockPos::new(0, 0, 0),
            game_ticks: 0,
            has_delayed_destroy: false,
            delayed_destroy_pos: BlockPos::new(0, 0, 0),
            delayed_tick_start: 0,
            last_sent_state: -1,
        }
    }

    /// Ticks the block breaking manager.
    ///
    /// This handles delayed destruction and updates break progress.
    pub fn tick(&mut self, player: &Player, world: &World) {
        self.game_ticks += 1;

        if self.has_delayed_destroy {
            let state = world.get_block_state(&self.delayed_destroy_pos);
            if is_air(state) {
                self.has_delayed_destroy = false;
            } else {
                let progress = self.increment_destroy_progress(
                    player,
                    world,
                    state,
                    self.delayed_destroy_pos,
                    self.delayed_tick_start,
                );
                if progress >= 1.0 {
                    self.has_delayed_destroy = false;
                    self.destroy_block(player, world, self.delayed_destroy_pos);
                }
            }
        } else if self.is_destroying_block {
            let state = world.get_block_state(&self.destroy_pos);
            if is_air(state) {
                // Block was broken by something else
                world.broadcast_block_destruction(player.id, self.destroy_pos, -1);
                self.last_sent_state = -1;
                self.is_destroying_block = false;
            } else {
                self.increment_destroy_progress(
                    player,
                    world,
                    state,
                    self.destroy_pos,
                    self.destroy_progress_start,
                );
            }
        }
    }

    /// Calculates and updates destruction progress, broadcasting to clients.
    fn increment_destroy_progress(
        &mut self,
        player: &Player,
        world: &World,
        block_state: BlockStateId,
        pos: BlockPos,
        destroy_start_tick: u64,
    ) -> f32 {
        let ticks_spent = self.game_ticks.saturating_sub(destroy_start_tick);
        let destroy_speed = get_destroy_progress(player, block_state);
        let progress = destroy_speed * (ticks_spent + 1) as f32;
        let state = (progress * 10.0) as i32;

        if state != self.last_sent_state {
            world.broadcast_block_destruction(player.id, pos, state);
            self.last_sent_state = state;
        }

        progress
    }

    /// Handles a block break action from the client.
    ///
    /// Note: The caller (packet handler) is responsible for calling `ack_block_changes_up_to`
    /// after this method returns, matching vanilla behavior.
    #[allow(clippy::too_many_lines)]
    pub fn handle_block_break_action(
        &mut self,
        player: &Player,
        world: &World,
        pos: BlockPos,
        action: BlockBreakAction,
        _direction: Direction,
    ) {
        // Validate interaction range
        if !player.is_within_block_interaction_range(&pos) {
            return;
        }

        // Validate Y coordinate
        if pos.y() >= world.max_build_height() {
            player.connection.send_packet(CBlockUpdate {
                pos,
                block_state: world.get_block_state(&pos),
            });
            return;
        }

        match action {
            BlockBreakAction::Start => {
                // Check may_interact permission
                if !world.may_interact(player, &pos) {
                    player.connection.send_packet(CBlockUpdate {
                        pos,
                        block_state: world.get_block_state(&pos),
                    });
                    return;
                }

                // Creative mode: instant break
                if player.game_mode.load() == GameType::Creative {
                    self.destroy_and_ack(player, world, pos);
                    return;
                }

                // Check if player can break this block (adventure mode restrictions, etc.)
                // TODO: Implement blockActionRestricted check

                self.destroy_progress_start = self.game_ticks;
                let block_state = world.get_block_state(&pos);

                if !is_air(block_state) {
                    // TODO: Call EnchantmentHelper.onHitBlock and blockState.attack

                    let progress = get_destroy_progress(player, block_state);

                    if progress >= 1.0 {
                        // Insta-mine
                        self.destroy_and_ack(player, world, pos);
                    } else {
                        // Start breaking
                        if self.is_destroying_block {
                            // Send block update for the old position to cancel client prediction
                            player.connection.send_packet(CBlockUpdate {
                                pos: self.destroy_pos,
                                block_state: world.get_block_state(&self.destroy_pos),
                            });
                        }

                        self.is_destroying_block = true;
                        self.destroy_pos = pos;
                        let state = (progress * 10.0) as i32;
                        world.broadcast_block_destruction(player.id, pos, state);
                        self.last_sent_state = state;
                    }
                }
            }

            BlockBreakAction::Stop => {
                if pos == self.destroy_pos {
                    let ticks_spent = self.game_ticks.saturating_sub(self.destroy_progress_start);
                    let block_state = world.get_block_state(&pos);

                    if !is_air(block_state) {
                        let destroy_speed = get_destroy_progress(player, block_state);
                        let progress = destroy_speed * (ticks_spent + 1) as f32;

                        if progress >= 0.7 {
                            // Complete the break
                            self.is_destroying_block = false;
                            world.broadcast_block_destruction(player.id, pos, -1);
                            self.destroy_and_ack(player, world, pos);
                            return;
                        }

                        if !self.has_delayed_destroy {
                            // Set up delayed destroy
                            self.is_destroying_block = false;
                            self.has_delayed_destroy = true;
                            self.delayed_destroy_pos = pos;
                            self.delayed_tick_start = self.destroy_progress_start;
                        }
                    }
                }
            }

            BlockBreakAction::Abort => {
                self.is_destroying_block = false;

                if self.destroy_pos != pos {
                    log::warn!(
                        "Mismatch in destroy block pos: {:?} vs {:?}",
                        self.destroy_pos,
                        pos
                    );
                    world.broadcast_block_destruction(player.id, self.destroy_pos, -1);
                }

                world.broadcast_block_destruction(player.id, pos, -1);
            }
        }
    }

    /// Destroys a block and sends appropriate response.
    fn destroy_and_ack(&mut self, player: &Player, world: &World, pos: BlockPos) {
        if !self.destroy_block(player, world, pos) {
            // Send block update to resync client
            player.connection.send_packet(CBlockUpdate {
                pos,
                block_state: world.get_block_state(&pos),
            });
        }
    }

    /// Destroys a block at the given position.
    ///
    /// Returns true if the block was successfully destroyed.
    #[allow(clippy::unused_self)]
    fn destroy_block(&self, player: &Player, world: &World, pos: BlockPos) -> bool {
        let state = world.get_block_state(&pos);

        // Check if player's tool can destroy this block
        // TODO: Implement canDestroyBlock check for adventure mode

        // Get block info
        let Some(_block) = REGISTRY.blocks.by_state_id(state) else {
            return false;
        };

        // TODO: Check for GameMasterBlock (command blocks, etc.)
        // TODO: Check blockActionRestricted

        // Remove the block
        let air_state = REGISTRY.blocks.get_base_state_id(vanilla_blocks::AIR);
        let changed = world.set_block(pos, air_state, UpdateFlags::UPDATE_ALL);

        if changed {
            // Play block destruction particles and sound (skip for fire blocks like vanilla)
            // Exclude the breaking player as they see the effect client-side
            let block = REGISTRY.blocks.by_state_id(state);
            let is_fire = block.is_some_and(|b| {
                b.key == vanilla_blocks::FIRE.key || b.key == vanilla_blocks::SOUL_FIRE.key
            });
            if !is_fire {
                world.destroy_block_effect(pos, u32::from(state.0), Some(player.id));
            }

            // Check if player has correct tool for drops
            let has_correct_tool = {
                let inv = player.inventory.lock();
                let main_hand = inv.get_item_in_hand(InteractionHand::MainHand);
                main_hand.is_correct_tool_for_drops(state) || !requires_correct_tool(state)
            };

            // Damage the tool if the block has non-zero destroy time
            // This is done before playerDestroy, matching vanilla's Item.mineBlock
            let block_destroy_time = REGISTRY
                .blocks
                .by_state_id(state)
                .map_or(0.0, |b| b.config.destroy_time);

            if block_destroy_time != 0.0 {
                let mut inv = player.inventory.lock();
                let damage_per_block = inv.get_selected_item().get_tool_damage_per_block();

                if damage_per_block > 0 {
                    // Use with_selected_item_mut to ensure set_changed() is called
                    // Skip damage if player has infinite materials (creative mode)
                    let has_infinite_materials = player.has_infinite_materials();
                    let broke = inv.with_selected_item_mut(|main_hand| {
                        main_hand.hurt_and_break(damage_per_block, has_infinite_materials)
                    });
                    if broke {
                        // TODO: Play item break sound/particles
                        log::debug!("Tool broke while mining block at {pos:?}");
                    }
                }
            }

            // Handle drops (skip for creative/spectator)
            let game_mode = player.game_mode.load();
            if game_mode != GameType::Spectator
                && game_mode != GameType::Creative
                && has_correct_tool
            {
                // TODO: Call playerDestroy to spawn drops
                drop_block_loot(player, world, pos, state);
            }
        }

        changed
    }
}

/// Block break action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockBreakAction {
    /// Player started breaking a block.
    Start,
    /// Player stopped breaking a block (finished or released).
    Stop,
    /// Player aborted breaking a block.
    Abort,
}

/// Checks if a block state is air.
fn is_air(state: BlockStateId) -> bool {
    let Some(block) = REGISTRY.blocks.by_state_id(state) else {
        return true;
    };
    block.config.is_air
}

/// Checks if a block requires the correct tool for drops.
fn requires_correct_tool(state: BlockStateId) -> bool {
    let Some(block) = REGISTRY.blocks.by_state_id(state) else {
        return false;
    };
    block.config.requires_correct_tool_for_drops
}

/// Gets the destroy progress per tick for a block.
///
/// This is based on the vanilla formula:
/// `1.0 / (destroy_time * 30.0)` for survival with correct tool
/// `1.0 / (destroy_time * 100.0)` for survival with wrong tool
/// Instant break for creative mode.
fn get_destroy_progress(player: &Player, block_state: BlockStateId) -> f32 {
    let Some(block) = REGISTRY.blocks.by_state_id(block_state) else {
        return 0.0;
    };

    let destroy_time = block.config.destroy_time;

    // Instant break for creative
    if player.game_mode.load() == GameType::Creative {
        return 1.0;
    }

    // Unbreakable block
    if destroy_time < 0.0 {
        return 0.0;
    }

    // Instant break for destroy_time == 0
    if destroy_time == 0.0 {
        return 1.0;
    }

    // Get player's mining speed
    let mining_speed = {
        let inv = player.inventory.lock();
        let main_hand = inv.get_item_in_hand(InteractionHand::MainHand);
        main_hand.get_destroy_speed(block_state)
    };

    // Check if player has the correct tool
    let has_correct_tool = {
        let inv = player.inventory.lock();
        let main_hand = inv.get_item_in_hand(InteractionHand::MainHand);
        main_hand.is_correct_tool_for_drops(block_state)
    };

    // Apply speed modifiers
    let speed = mining_speed;

    // TODO: Apply efficiency enchantment
    // TODO: Apply haste/mining fatigue effects
    // TODO: Apply underwater/in-air penalties

    // Calculate destroy progress per tick
    // Vanilla formula: speed / hardness / (hasCorrectTool ? 30 : 100)
    let divisor = if has_correct_tool || !block.config.requires_correct_tool_for_drops {
        30.0
    } else {
        100.0
    };

    speed / destroy_time / divisor
}

/// Drops loot for a destroyed block using its loot table.
#[allow(clippy::needless_pass_by_value)]
fn drop_block_loot(player: &Player, _world: &World, pos: BlockPos, state: BlockStateId) {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::loot_table::LootContext;
    use steel_utils::Identifier;

    let block = state.get_block();

    // Build the loot table key: "blocks/{block_name}"
    let loot_table_key = Identifier::vanilla(format!("blocks/{}", block.key.path));

    let Some(loot_table) = REGISTRY.loot_tables.by_key(&loot_table_key) else {
        // No loot table for this block (e.g., air, bedrock)
        return;
    };

    // Get the player's tool
    let tool = player.inventory.lock().get_selected_item().clone();

    // Create loot context
    let mut rng = rand::rng();
    let mut ctx = LootContext {
        rng: &mut rng,
        luck: 0.0, // TODO: Get luck from player attributes
        block_state: Some(state),
        tool: Some(&tool),
        explosion_radius: None,
        killed_by_player: false,
        origin: Some((f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()))),
        game_time: None,
        weather: None,
        this_entity: None,
        killer_entity: None,
        direct_killer_entity: None,
        last_damage_player: None,
        damage_source: None,
        block_entity: None,
        interacting_entity: None,
    };

    // Generate drops
    let drops = loot_table.get_random_items(&mut ctx);

    // Spawn each dropped item using the player's world reference (Arc<World>)
    for item in drops {
        if !item.is_empty() {
            player.world.pop_resource(&pos, item);
        }
    }
}
