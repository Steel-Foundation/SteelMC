use super::{
    ADVANCE_TIME, BlockPos, CChangeDifficulty, ChunkPos, Difficulty, Digest, ErasedGameRuleRef,
    GameRule, GameRuleValue, GameRuleValueType, LevelDataManager, OffsetVoxelShape, Ordering,
    Player, REGISTRY, SectionPos, Sha256, World, vanilla_dimension_types,
};

impl World {
    /// Vanilla `Level.MAX_LEVEL_SIZE`.
    pub const MAX_LEVEL_SIZE: i32 = 30_000_000;
    /// Vanilla `Level.MAX_ENTITY_SPAWN_Y`.
    pub const MAX_ENTITY_SPAWN_Y: i32 = 20_000_000;
    /// Vanilla `Level.MIN_ENTITY_SPAWN_Y`.
    pub const MIN_ENTITY_SPAWN_Y: i32 = -20_000_000;

    /// Returns whether this world uses the vanilla End dimension type.
    ///
    /// Steel world keys are domain-scoped, so End gameplay semantics cannot rely on the
    /// vanilla `minecraft:the_end` level key.
    #[must_use]
    pub fn is_end_dimension_type(&self) -> bool {
        self.dimension_type == &vanilla_dimension_types::THE_END
    }

    /// Returns vanilla level difficulty.
    pub fn difficulty(&self) -> Difficulty {
        self.level_data.read().data().difficulty
    }

    /// Sets the level difficulty and broadcasts the new value to its players.
    pub(crate) fn set_difficulty(&self, difficulty: Difficulty) {
        let locked = {
            let mut level_data = self.level_data.write();
            level_data.data_mut().difficulty = difficulty;
            level_data.data().difficulty_locked
        };
        self.broadcast_to_all(CChangeDifficulty { difficulty, locked });
    }

    /// Returns the total height of the world in blocks.
    pub const fn get_height(&self) -> i32 {
        self.dimension_type.height
    }

    /// Returns the minimum Y coordinate of the world.
    pub const fn get_min_y(&self) -> i32 {
        self.dimension_type.min_y
    }

    /// Returns the maximum Y coordinate of the world.
    pub const fn get_max_y(&self) -> i32 {
        self.get_min_y() + self.get_height() - 1
    }

    /// Returns whether the given Y coordinate is outside the build height.
    pub const fn is_outside_build_height(&self, block_y: i32) -> bool {
        block_y < self.get_min_y() || block_y > self.get_max_y()
    }

    /// Returns whether the block position is within valid horizontal bounds.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub const fn is_in_valid_bounds_horizontal(&self, block_pos: BlockPos) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(block_pos.0.x);
        let chunk_z = SectionPos::block_to_section_coord(block_pos.0.z);
        ChunkPos::is_valid(chunk_x, chunk_z)
    }

    /// Returns whether the block position is within valid world bounds.
    pub const fn is_in_valid_bounds(&self, block_pos: BlockPos) -> bool {
        !self.is_outside_build_height(block_pos.0.y)
            && self.is_in_valid_bounds_horizontal(block_pos)
    }

    /// Returns whether a block position is within vanilla's absolute world bounds.
    pub const fn is_in_world_bounds(&self, block_pos: BlockPos) -> bool {
        !self.is_outside_build_height(block_pos.0.y)
            && Self::is_in_world_bounds_horizontal(block_pos)
    }

    /// Returns whether the block position is within vanilla spawnable bounds.
    #[must_use]
    pub const fn is_in_spawnable_bounds(block_pos: BlockPos) -> bool {
        !Self::is_outside_spawnable_height(block_pos.0.y)
            && Self::is_in_world_bounds_horizontal(block_pos)
    }

    pub(super) const fn is_in_world_bounds_horizontal(block_pos: BlockPos) -> bool {
        block_pos.0.x >= -Self::MAX_LEVEL_SIZE
            && block_pos.0.z >= -Self::MAX_LEVEL_SIZE
            && block_pos.0.x < Self::MAX_LEVEL_SIZE
            && block_pos.0.z < Self::MAX_LEVEL_SIZE
    }

    pub(super) const fn is_outside_spawnable_height(y: i32) -> bool {
        y < Self::MIN_ENTITY_SPAWN_Y || y >= Self::MAX_ENTITY_SPAWN_Y
    }

    /// Returns the maximum build height (one above the highest placeable block).
    /// This is `min_y + height`.
    #[must_use]
    pub const fn max_build_height(&self) -> i32 {
        self.get_min_y() + self.get_height()
    }

    /// Checks if a player may interact with the world at the given position.
    /// Currently only checks if position is within world bounds.
    #[must_use]
    pub const fn may_interact(&self, _player: &Player, pos: BlockPos) -> bool {
        self.is_in_valid_bounds(pos)
    }

    /// Checks if a block's collision shape at the given position is unobstructed by entities.
    ///
    /// This is the Rust equivalent of vanilla's `Level.isUnobstructed(BlockState, BlockPos, CollisionContext)`.
    /// In vanilla, this checks all entities with `blocksBuilding=true` (players, mobs, boats, etc.).
    ///
    /// Returns `true` if the position is clear, `false` if an entity would obstruct placement.
    #[must_use]
    pub fn is_unobstructed(&self, collision_shape: OffsetVoxelShape, pos: BlockPos) -> bool {
        if collision_shape.is_empty() {
            return true;
        }

        for block_aabb in collision_shape.iter() {
            let world_aabb = block_aabb.at_block(pos);
            for entity in self.get_entities_in_aabb(&world_aabb) {
                if entity.blocks_building() && entity.bounding_box().intersects(world_aabb) {
                    return false;
                }
            }
        }

        true
    }

    /// Returns whether the tick rate is running normally.
    ///
    /// When false (frozen/paused), movement validation checks should be skipped.
    /// Matches vanilla's `level.tickRateManager().runsNormally()`.
    #[must_use]
    pub fn tick_runs_normally(&self) -> bool {
        self.tick_runs_normally.load(Ordering::Relaxed)
    }

    /// Sets whether the tick rate is running normally.
    ///
    /// Set to false to freeze/pause the world (e.g., via `/tick freeze` command).
    pub fn set_tick_runs_normally(&self, runs_normally: bool) {
        self.tick_runs_normally
            .store(runs_normally, Ordering::Relaxed);
    }

    /// Mirrors `ServerLevel.isHandlingTick` for piston early-retraction rules.
    #[must_use]
    pub(crate) fn is_handling_tick(&self) -> bool {
        self.handling_tick.load(Ordering::Relaxed)
    }

    /// Gets the value of a game rule.
    /// WARNING: this function acquires a read lock on the level data.
    /// if you already have a write lock on level data, this will DEADLOCK
    #[must_use]
    pub fn get_game_rule<T: GameRuleValueType>(&self, rule: &GameRule<T>) -> T {
        let guard = self.level_data.read();
        self.get_game_rule_with_guard(rule, &guard)
    }

    /// Gets the value of a game rule on the `LevelDataManager` guard being passed in.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    #[must_use]
    pub fn get_game_rule_with_guard<T: GameRuleValueType>(
        &self,
        rule: &GameRule<T>,
        guard: &LevelDataManager,
    ) -> T {
        guard
            .data()
            .game_rules_values
            .get(rule, &REGISTRY.game_rules)
    }

    /// Gets a type-erased value for a dynamically selected game rule.
    #[must_use]
    pub fn get_erased_game_rule(&self, rule: ErasedGameRuleRef) -> GameRuleValue {
        self.level_data
            .read()
            .data()
            .game_rules_values
            .get_erased(rule, &REGISTRY.game_rules)
            .clone()
    }

    /// Sets the value of a game rule.
    /// WARNING: this function acquires a write lock on the level data.
    /// if you already have a read or write lock on level data, this will DEADLOCK
    pub fn set_game_rule<T: GameRuleValueType>(&self, rule: &GameRule<T>, value: T) -> bool {
        let updated = {
            let mut guard = self.level_data.write();
            self.set_game_rule_with_guard(rule, value, &mut guard)
        };
        if updated && rule.key() == ADVANCE_TIME.key() {
            self.broadcast_time_sync();
        }
        updated
    }

    /// Sets the value of a game rule on the `LevelDataManager` guard being passed in.
    #[expect(clippy::unused_self, reason = "this is an api function")]
    pub fn set_game_rule_with_guard<T: GameRuleValueType>(
        &self,
        rule: &GameRule<T>,
        value: T,
        guard: &mut LevelDataManager,
    ) -> bool {
        guard
            .data_mut()
            .game_rules_values
            .set(rule, value, &REGISTRY.game_rules)
    }

    /// Sets a type-erased value for a dynamically selected game rule.
    pub fn set_erased_game_rule(&self, rule: ErasedGameRuleRef, value: GameRuleValue) -> bool {
        let updated = self
            .level_data
            .write()
            .data_mut()
            .game_rules_values
            .set_erased(rule, value, &REGISTRY.game_rules);
        if updated && rule.key() == ADVANCE_TIME.key() {
            self.broadcast_time_sync();
        }
        updated
    }

    pub(super) fn advance_time_with_guard(&self, guard: &LevelDataManager) -> bool {
        self.get_game_rule_with_guard(&ADVANCE_TIME, guard)
    }

    /// Gets the world seed.
    #[must_use]
    pub fn seed(&self) -> i64 {
        self.level_data.read().data().seed
    }

    /// Gets the obfuscated seed for sending to clients.
    ///
    /// This uses SHA-256 hashing to prevent clients from easily extracting
    /// the actual world seed, matching vanilla's `BiomeManager.obfuscateSeed()`.
    #[must_use]
    #[expect(
        clippy::missing_panics_doc,
        reason = "panic is unreachable: SHA-256 always produces 32 bytes"
    )]
    pub fn obfuscated_seed(&self) -> i64 {
        let seed = self.seed();
        let mut hasher = Sha256::new();
        hasher.update(seed.to_be_bytes());
        let result = hasher.finalize();
        // SHA-256 always produces 32 bytes, so taking 8 bytes always succeeds
        let bytes: [u8; 8] = result[0..8].try_into().expect("SHA-256 produces 32 bytes");
        i64::from_be_bytes(bytes)
    }
}
