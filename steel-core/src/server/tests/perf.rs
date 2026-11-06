//! End-to-end coverage for `/perf start` → JSON report persistence.

use std::{path::Path, sync::Arc, time::Duration};

use tokio::{
    fs,
    runtime::Builder,
    time::{sleep, timeout},
};

use super::{run_command, test_server, test_storage_root};
use crate::{
    chunk::chunk_map::ChunkMapGameTickTimings,
    command::{execution::CommandSource, sender::CommandSender},
    permission::PermissionSubjectIndex,
    test_support::fresh_test_world,
    world::WorldGameTickTimings,
};

#[test]
fn perf_start_stop_writes_a_json_report() {
    let world_fixture = fresh_test_world("perf-command");
    let world = &world_fixture.world;
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let server = test_server(Arc::clone(world), PermissionSubjectIndex::new()).await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let results_dir = test_storage_root("perf-report");
        server.perf.set_results_dir(results_dir.clone());

        let source = CommandSource::new(CommandSender::Console, Arc::clone(&server));
        run_command(&server, source.clone(), "perf start");

        let key = world.key.clone();
        let timings = WorldGameTickTimings {
            elapsed: Duration::from_millis(42),
            chunk_map: ChunkMapGameTickTimings::default(),
            entity_tick: Duration::from_millis(3),
        };
        assert!(
            server
                .perf
                .record_tick(Duration::from_millis(42), [(&key, timings)])
                .is_none()
        );

        run_command(&server, source, "perf stop");

        let report = wait_for_report(&results_dir).await;
        assert_eq!(report["ticks"], 1);
        let world_key = world.key.to_string();
        let world_report = &report["worlds"][world_key.as_str()];
        assert!(
            world_report["tick_time"]["mean_ms"]
                .as_f64()
                .is_some_and(|value| (value - 42.0).abs() < f64::EPSILON)
        );
        assert!(world_report["chunk_map"]["tick_chunks"].is_object());
        assert!(world_report["counts"]["tickable_chunks"].is_object());
        assert!(world_report["counts"]["readiness_lookup_evictions"].is_object());

        fs::remove_dir_all(results_dir)
            .await
            .expect("test report directory should be removable");
    });
}

/// Polls `directory` until the spawned report task writes a complete JSON document.
async fn wait_for_report(directory: &Path) -> serde_json::Value {
    let result = timeout(Duration::from_secs(5), async {
        loop {
            let Ok(mut entries) = fs::read_dir(directory).await else {
                sleep(Duration::from_millis(10)).await;
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                let Ok(bytes) = fs::read(&path).await else {
                    continue;
                };
                if let Ok(report) = serde_json::from_slice(&bytes) {
                    return report;
                }
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    result.expect("performance report should be written")
}
