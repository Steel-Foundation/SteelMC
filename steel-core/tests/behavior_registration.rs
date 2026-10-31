//! Pre-init subscribes events; the existing initializer edits and freezes the registry once.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use steel_core::{
    behavior::{BLOCK_BEHAVIORS, BlockBehavior, BlockBehaviorRegistry, BlockPlaceContext},
    bootstrap::init_globals,
};
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::{BlockStateId, locks::SyncMutex};
struct ReplacedBehavior(Arc<dyn BlockBehavior>);
impl BlockBehavior for ReplacedBehavior {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.0.get_state_for_placement(context)
    }
    fn is_bed(&self) -> bool {
        true
    }
}
#[test]
fn pre_init_only_subscribes_and_init_runs_registration_once() -> Result<(), String> {
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(SyncMutex::new(None));
    let observed_calls = Arc::clone(&calls);
    let observed_capture = Arc::clone(&captured);
    BlockBehaviorRegistry::on_registration(move |registry| {
        assert!(REGISTRY.is_initialized());
        assert!(BLOCK_BEHAVIORS.get().is_none());
        let block = &vanilla_blocks::WHITE_WOOL;
        let original = registry.get_behavior_shared(block);
        registry.set_behavior(block, Box::new(ReplacedBehavior(Arc::clone(&original))));
        assert!(registry.get_behavior(block).is_bed());
        *observed_capture.lock() = Some(original);
        observed_calls.fetch_add(1, Ordering::Relaxed);
    })?;
    assert!(!REGISTRY.is_initialized());
    assert!(BLOCK_BEHAVIORS.get().is_none());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    init_globals();
    assert!(
        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::WHITE_WOOL)
            .is_bed()
    );
    assert!(
        !captured
            .lock()
            .as_ref()
            .ok_or("registration did not run")?
            .is_bed()
    );
    init_globals();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(BlockBehaviorRegistry::on_registration(|_| {}).is_err());
    Ok(())
}
