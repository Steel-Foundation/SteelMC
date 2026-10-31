use super::{
    BlockBehavior, BlockPlaceContext, BlockPos, BlockRef, BlockStateId, Direction, REGISTRY,
    RegistryEntry, RegistryExt, ScheduledTickAccess, schedule_water_tick_if_waterlogged,
};
use std::sync::Arc;
use steel_utils::locks::SyncMutex;

type RegistrationHook = Box<dyn FnOnce(&mut BlockBehaviorRegistry) + Send>;
static REGISTRATION_HOOKS: SyncMutex<Option<Vec<RegistrationHook>>> =
    SyncMutex::new(Some(Vec::new()));

/// Default placement plus the common water tick used by unported waterlogged blocks.
pub struct DefaultBlockBehavior {
    block: BlockRef,
}

impl DefaultBlockBehavior {
    /// Creates a new default block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for DefaultBlockBehavior {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        state
    }

    // This fallback only preserves generic block placement. Blocks with
    // class-specific placement rules still need their own behavior.
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}

/// Registry for block behaviors.
///
/// Created after the main registry is frozen. All blocks are initialized with
/// default behaviors, then custom behaviors are registered for specific blocks.
pub struct BlockBehaviorRegistry {
    behaviors: Vec<Arc<dyn BlockBehavior>>,
}

impl BlockBehaviorRegistry {
    /// Subscribes before initialization; the callback edits the live registry before freezing.
    pub fn on_registration(hook: impl FnOnce(&mut Self) + Send + 'static) -> Result<(), String> {
        let mut hooks = REGISTRATION_HOOKS.lock();
        let Some(hooks) = hooks.as_mut() else {
            return Err("block behavior registration has already finished".into());
        };
        hooks.push(Box::new(hook));
        Ok(())
    }

    pub(crate) fn run_registration_hooks(&mut self) {
        let hooks = REGISTRATION_HOOKS.lock().take();
        if let Some(hooks) = hooks {
            for hook in hooks {
                hook(self);
            }
        }
    }

    /// Get all behaviors.
    #[cfg(feature = "test-framework")]
    #[must_use]
    pub fn get_behaviors(&self) -> &[Arc<dyn BlockBehavior>] {
        &self.behaviors
    }

    /// Creates a new behavior registry with default behaviors for all blocks.
    #[must_use]
    pub fn new() -> Self {
        let block_count = REGISTRY.blocks.len();
        let mut behaviors: Vec<Arc<dyn BlockBehavior>> = Vec::with_capacity(block_count);

        // Initialize all blocks with default behavior
        for (_, block) in REGISTRY.blocks.iter() {
            behaviors.push(Arc::new(DefaultBlockBehavior::new(block)));
        }

        Self { behaviors }
    }

    /// Sets a custom behavior for a block.
    pub fn set_behavior(&mut self, block: BlockRef, behavior: Box<dyn BlockBehavior>) {
        let id = block.id();
        self.behaviors[id] = Arc::from(behavior);
    }

    /// Gets the behavior for a block.
    #[must_use]
    pub fn get_behavior(&self, block: BlockRef) -> &dyn BlockBehavior {
        self.behaviors[block.id()].as_ref()
    }

    /// Retains the current implementation independently of later registry edits.
    #[must_use]
    pub fn get_behavior_shared(&self, block: BlockRef) -> Arc<dyn BlockBehavior> {
        Arc::clone(&self.behaviors[block.id()])
    }

    /// Gets the behavior for a block state.
    #[must_use]
    pub fn get_behavior_for_state(&self, state: BlockStateId) -> Option<&dyn BlockBehavior> {
        let block = REGISTRY.blocks.by_state_id(state)?;
        Some(self.get_behavior(block))
    }
}

impl Default for BlockBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
