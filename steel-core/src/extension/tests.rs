use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    behavior::{BlockBehavior, BlockBehaviorRegistry, BlockPlaceContext, DefaultBlockBehavior},
    bootstrap::init_globals,
    test_support::fresh_test_world,
};
use steel_registry::vanilla_blocks;
use steel_utils::BlockStateId;

struct ReplacedBehavior {
    original: Arc<dyn BlockBehavior>,
}

impl BlockBehavior for ReplacedBehavior {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.original.get_state_for_placement(context)
    }

    fn is_bed(&self) -> bool {
        true
    }
}

#[test]
fn complete_replacement_updates_the_vector_and_keeps_the_original_available() {
    init_globals();
    let mut registry = BlockBehaviorRegistry::new();
    let block = &vanilla_blocks::WHITE_WOOL;
    registry.set_behavior(block, Box::new(DefaultBlockBehavior::new(block)));
    let original = registry.get_behavior_shared(block);
    registry.set_behavior(
        block,
        Box::new(ReplacedBehavior {
            original: Arc::clone(&original),
        }),
    );
    assert!(registry.get_behavior(block).is_bed());
    assert!(!original.is_bed());
    assert_eq!(
        registry
            .get_behavior_for_state(block.default_state())
            .map(BlockBehavior::is_bed),
        Some(true),
    );
    registry.set_behavior(block, Box::new(DefaultBlockBehavior::new(block)));
    assert!(!registry.get_behavior(block).is_bed());
    assert!(!original.is_bed());
}

struct TickExtension(Arc<AtomicUsize>);
impl WorldTickHandler for TickExtension {
    fn tick(&self, _world: &Arc<World>, _tick_count: u64) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn frozen_world_ticks_do_not_invoke_gameplay_extensions() {
    init_globals();
    let fixture = fresh_test_world("extension_frozen_tick");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut extensions = Extensions::default();
    extensions
        .hooks
        .register_world_tick(Box::new(TickExtension(Arc::clone(&calls))));
    assert!(fixture.world.extensions.set(Arc::new(extensions)).is_ok());
    fixture.world.tick_game(1, false);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    fixture.world.tick_game(2, true);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

struct StopExtension(Arc<AtomicUsize>);
impl ServerStopHandler for StopExtension {
    fn stop(&self, _server: &Server) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn shutdown_notifies_extensions_once_even_if_shutdown_is_requested_again() -> Result<(), String> {
    use crate::{
        config::ResolvedDomainConfig, permission::PermissionSubjectIndex,
        server::test_server_with_worlds_and_config, test_support::test_runtime_config,
    };
    use std::slice::from_ref;
    use tokio::runtime::Builder;

    init_globals();
    let fixture = fresh_test_world("extension_shutdown");
    let domain = ResolvedDomainConfig {
        name: fixture.world.domain().to_owned(),
        default_world: fixture.world.key.clone(),
        worlds: vec![fixture.world.key.clone()],
    };
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let server = runtime.block_on(test_server_with_worlds_and_config(
        domain.name.clone(),
        from_ref(&domain),
        from_ref(&fixture.world),
        PermissionSubjectIndex::new(),
        test_runtime_config(1),
        None,
    ))?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut hooks = ExtensionHooks::default();
    hooks.register_server_stop(Box::new(StopExtension(Arc::clone(&calls))));
    hooks.stop_server(&server);
    hooks.stop_server(&server);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}
