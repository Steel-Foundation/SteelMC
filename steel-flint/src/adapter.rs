//! Flint adapter implementation for `SteelMC`.
//!
//! This module provides the [`SteelAdapter`] which implements the `FlintAdapter` trait,
//! allowing the Flint testing framework to create test worlds using the real steel-core
//! World implementation.

use crate::world::SteelTestWorld;
use anyhow::Context;
use dotenvy::dotenv;
use flint_core::results::TestSummary;
use flint_core::test_spec::MinimalTestSpec;
use flint_core::{FlintAdapter, FlintWorld, ServerInfo, TestLoader};
use rustc_hash::FxHashSet;
use serde::Deserialize;
use std::any::type_name;
use std::env::var;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use steel_core::behavior::block_behaviors::PLAIN_BLOCKS;
use steel_core::behavior::item_behaviors::PLAIN_ITEMS;
use steel_core::behavior::{BLOCK_BEHAVIORS, DefaultItemBehavior, ITEM_BEHAVIORS};
use steel_core::entity::ENTITIES;
use steel_registry::blocks::BlockRef;
use steel_registry::{REGISTRY, RegistryExt};
use toml::from_str;

/// Adapter for running Flint tests against `SteelMC`.
///
/// Call `steel_flint::init()` before creating test worlds.
#[derive(Clone, Default)]
pub struct SteelAdapter;

impl FlintAdapter for SteelAdapter {
    fn create_test_world(&self) -> Result<Box<dyn FlintWorld + 'static>, anyhow::Error> {
        Ok(Box::new(SteelTestWorld::new()?))
    }

    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            minecraft_version: steel_utils::MC_VERSION.to_string(),
        }
    }
}

fn implemented_blocks() -> impl Iterator<Item = BlockRef> {
    BLOCK_BEHAVIORS
        .get_behaviors()
        .iter()
        .enumerate()
        .filter(|(_, behavior)| !behavior.type_name().ends_with("DefaultBlockBehavior"))
        .filter_map(|(id, _)| REGISTRY.blocks.by_id(id))
        .chain(PLAIN_BLOCKS.iter().copied())
}

/// Returns IDs of blocks with a non-default behavior or a plain vanilla `Block` class.
#[must_use]
pub fn get_implemented_blocks() -> Vec<String> {
    implemented_blocks()
        .map(|block| block.key.to_string())
        .collect()
}

/// Returns IDs of items whose item behavior and any placed target block are implemented.
#[must_use]
pub fn get_implemented_items() -> Vec<String> {
    let blocks: FxHashSet<_> = implemented_blocks().map(|block| &block.key).collect();
    ITEM_BEHAVIORS
        .get_behaviors()
        .iter()
        .enumerate()
        .filter_map(|(id, behavior)| {
            let item = REGISTRY.items.by_id(id)?;
            if behavior.type_name() == type_name::<DefaultItemBehavior>() {
                return None;
            }
            item.block
                .is_none_or(|block| blocks.contains(&block.key))
                .then_some(item)
        })
        .chain(PLAIN_ITEMS.iter().copied().map(LazyLock::force))
        .map(|item| item.key.to_string())
        .collect()
}

/// Returns IDs of entities with an implementation.
pub fn get_implemented_entities() -> Vec<String> {
    ENTITIES
        .get_implemented()
        .map(|entity| entity.key.to_string())
        .collect()
}

/// Flint run settings loaded from `flint.toml`.
#[derive(Deserialize, Default)]
pub struct FlintConfig {
    filter: Option<FilterConfig>,
    /// Base URL of the flint-viz instance to embed in failure URLs.
    /// Priority: `FLINT_VIZ_URL` env var > toml `viz_url` > default.
    #[serde(alias = "flint_viz_url", alias = "FLINT_VIZ_URL")]
    viz_url: Option<String>,
    /// Directory containing the flint tests.
    /// Priority: `TEST_PATH` env var > toml `test_path` > `./test`.
    test_path: Option<String>,
}

impl FlintConfig {
    /// Resolves the test directory.
    #[must_use]
    pub fn test_path(&self) -> PathBuf {
        var("TEST_PATH")
            .ok()
            .or_else(|| self.test_path.clone())
            .map_or_else(|| PathBuf::from("./test"), PathBuf::from)
    }
}

#[derive(Deserialize, Default)]
struct FilterConfig {
    tags: Option<rustc_hash::FxHashMap<String, bool>>,
    implemented_only: Option<bool>,
    test: Option<String>,
    pattern: Option<String>,
}

/// Loads `flint.toml` from the working directory, using defaults when the file is absent.
pub fn load_config() -> anyhow::Result<FlintConfig> {
    match fs::read_to_string("flint.toml") {
        Ok(contents) => from_str(&contents).context("Invalid flint.toml"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FlintConfig::default()),
        Err(error) => Err(error).context("Could not read flint.toml"),
    }
}

/// Loads environment variables from `.env`, if present.
pub fn init_env() -> anyhow::Result<()> {
    match dotenv() {
        Ok(_) => Ok(()),
        Err(error) if error.not_found() => Ok(()),
        Err(error) => Err(error).context("Could not load .env"),
    }
}

/// Collects test file paths based on environment variables and `flint.toml`.
/// Priority: `FLINT_TEST` > `FLINT_TAGS` > `implemented_only` (`FLINT_IMPLEMENTED_ONLY` env > toml) > `filter.tags` > `FLINT_PATTERN` > all
pub fn collect_filtered_paths(
    loader: &TestLoader,
    cfg: &FlintConfig,
) -> anyhow::Result<Vec<PathBuf>> {
    let filter = cfg.filter.as_ref();

    // Single test by name: env > toml
    let test_name = var("FLINT_TEST")
        .ok()
        .or_else(|| filter.and_then(|f| f.test.clone()));
    if let Some(name) = test_name {
        println!("Running single test: {name}");
        return Ok(loader
            .collect_all_test_files()?
            .into_iter()
            .filter(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n == name)
            })
            .collect());
    }

    // Tag filtering: env > implemented_only > toml tags
    let env_tags = var("FLINT_TAGS").ok().map(|s| {
        s.split(',')
            .map(|t| t.trim().to_string())
            .collect::<Vec<_>>()
    });
    let implemented_only = var("FLINT_IMPLEMENTED_ONLY").map_or_else(
        |_| filter.and_then(|f| f.implemented_only).unwrap_or(false),
        |v| matches!(v.as_str(), "1" | "true"),
    );
    if env_tags.is_none() && implemented_only {
        let mut ids = get_implemented_items();
        ids.extend(get_implemented_blocks());
        ids.extend(get_implemented_entities());
        let implemented: FxHashSet<_> = ids.into_iter().collect();
        let mut paths = Vec::new();
        for path in loader.collect_all_test_files()? {
            if all_ids_implemented(&path, &implemented)? {
                paths.push(path);
            }
        }
        println!(
            "Running {} test(s) whose minecraftIds are all implemented",
            paths.len()
        );
        return Ok(paths);
    }
    let tags = env_tags.or_else(|| {
        filter.and_then(|f| {
            f.tags.as_ref().map(|map| {
                map.iter()
                    .filter(|&(_, &enabled)| enabled)
                    .map(|(tag, _)| tag.clone())
                    .collect()
            })
        })
    });
    if let Some(tags) = tags {
        println!("Running tests with tags: {}", tags.join(", "));
        return Ok(loader.collect_by_tags(&tags));
    }

    // Pattern matching: env > toml
    let pattern = var("FLINT_PATTERN")
        .ok()
        .or_else(|| filter.and_then(|f| f.pattern.clone()));
    if let Some(p) = pattern {
        println!("Running tests matching pattern: {p}");
        return Ok(loader
            .collect_all_test_files()?
            .into_iter()
            .filter(|path| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|name| matches_pattern(name, &p))
            })
            .collect());
    }

    // Default: all tests
    println!("Running all flint tests");
    loader.collect_all_test_files()
}

/// Whether every `minecraftIds` entry of the test at `path` is implemented.
fn all_ids_implemented(path: &Path, implemented: &FxHashSet<String>) -> anyhow::Result<bool> {
    let json =
        fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))?;
    let spec: MinimalTestSpec = serde_json::from_str(&json)
        .with_context(|| format!("Invalid Flint test header in {}", path.display()))?;
    Ok(!spec.minecraft_ids.is_empty()
        && spec.minecraft_ids.iter().all(|id| implemented.contains(id)))
}

/// Simple glob pattern matching (supports * wildcard)
fn matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    name == pattern
}

/// Writes the CI summary to `log/flint_summary.json`.
pub fn save_summary(summary: &TestSummary) -> io::Result<()> {
    fs::create_dir_all("log")?;
    let entries: Vec<_> = summary
        .results
        .iter()
        .map(|result| {
            serde_json::json!({
                "name": result.test_name,
                "ids": result.minecraft_ids,
                "success": result.success && !result.skipped,
                "skipped": result.skipped,
                "error": result.failure_reason,
            })
        })
        .collect();
    fs::write(
        "log/flint_summary.json",
        serde_json::to_vec_pretty(&entries)?,
    )?;
    println!("Summary saved to log/flint_summary.json");
    Ok(())
}

fn resolve_viz_base_url(cfg: &FlintConfig) -> String {
    var("FLINT_VIZ_URL")
        .ok()
        .or_else(|| cfg.viz_url.clone())
        .unwrap_or_else(|| "https://flinttestmc.github.io/FlintViz".to_string())
}

/// For each failing test in `summary`, print a clickable flint-viz URL with
/// the failing assertion(s) + inline `TestSpec` baked in. Designed to land
/// directly below the failure tree so terminal emulators (and PR-comment
/// renderers) auto-link it.
pub fn print_failure_urls(
    specs: &[flint_core::TestSpecLoadResult],
    paths: &[PathBuf],
    summary: &TestSummary,
    cfg: &FlintConfig,
) {
    use flint_core::results::AssertionResult;
    use flint_core::viz_link::{FailurePayload, failure_url};

    let base = resolve_viz_base_url(cfg);
    let mut emitted = false;
    // Results follow spec order, including skips and any partial run that panicked.
    for ((load_result, path), result) in specs.iter().zip(paths.iter()).zip(summary.results.iter())
    {
        if result.success || result.skipped {
            continue;
        }
        let flint_core::TestSpecLoadResult::Loaded(spec) = load_result else {
            continue;
        };
        let failures: Vec<_> = result
            .assertions
            .iter()
            .filter_map(|a| match a {
                AssertionResult::Failure(f) => Some(f.clone()),
                AssertionResult::Success(_) => None,
            })
            .collect();
        if failures.is_empty() {
            continue;
        }
        let payload = FailurePayload::new(
            spec.clone(),
            Some(path.clone()),
            failures,
            result.total_ticks,
        );
        match failure_url(&payload, &base) {
            Ok(url) => {
                if !emitted {
                    println!();
                    emitted = true;
                }
                let name = spec.name.clone();
                println!("[{name}]Open in flint-viz: {url}");
            }
            Err(err) => {
                eprintln!(
                    "warning: could not encode flint-viz URL for `{}`: {err}",
                    result.test_name
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TestLoader, TestRunner};
    use std::sync::Arc;
    use steel_utils::locks::SyncMutex;

    static SUMMARY_LOCK: SyncMutex<()> = SyncMutex::new(());

    #[test]
    fn implemented_items_include_base_items_and_block_item_aliases() {
        crate::init();
        let blocks: FxHashSet<_> = get_implemented_blocks().into_iter().collect();
        let items: FxHashSet<_> = get_implemented_items().into_iter().collect();
        for id in [
            "minecraft:stick",
            "minecraft:carrot",
            "minecraft:cocoa_beans",
            "minecraft:glow_berries",
            "minecraft:oak_door",
            "minecraft:torch",
            "minecraft:lily_pad",
            "minecraft:scaffolding",
        ] {
            assert!(items.contains(id), "Missing {id}");
        }
        for id in ["minecraft:glass", "minecraft:frogspawn"] {
            assert_eq!(
                items.contains(id),
                blocks.contains(id),
                "Block-item gating mismatch for {id}"
            );
        }
    }

    #[test]
    #[ignore = "needs FlintBenchmark tests (TEST_PATH); run with --ignored"]
    fn test_run_flint_selected() -> anyhow::Result<()> {
        let _summary_guard = SUMMARY_LOCK.lock();
        let cfg = load_suite_config()?;
        let loader = TestLoader::new(&cfg.test_path(), true)?;
        run_suite(&cfg, &loader, collect_filtered_paths(&loader, &cfg)?)
    }

    #[test]
    #[ignore = "needs FlintBenchmark tests (TEST_PATH); run with --ignored"]
    fn test_run_all_flint_benchmarks() -> anyhow::Result<()> {
        let _summary_guard = SUMMARY_LOCK.lock();
        let cfg = load_suite_config()?;
        let loader = TestLoader::new(&cfg.test_path(), true)?;
        run_suite(&cfg, &loader, loader.collect_all_test_files()?)
    }

    fn load_suite_config() -> anyhow::Result<FlintConfig> {
        match fs::remove_file("log/flint_summary.json") {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Could not remove the previous Flint summary"),
        }
        crate::init();
        init_env()?;
        load_config()
    }

    fn run_suite(
        cfg: &FlintConfig,
        loader: &TestLoader,
        paths: Vec<PathBuf>,
    ) -> anyhow::Result<()> {
        use flint_core::TestSpecLoadResult;
        use flint_core::results::TestResult;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        anyhow::ensure!(
            !paths.is_empty(),
            "No Flint tests selected in {}",
            cfg.test_path().display()
        );
        let specs = loader.load_specs(&paths, false)?;
        let runner = TestRunner::new(Arc::new(SteelAdapter));
        let mut results = Vec::with_capacity(specs.len());
        for spec in &specs {
            match spec {
                TestSpecLoadResult::Loaded(spec) => {
                    let Ok(result) = catch_unwind(AssertUnwindSafe(|| runner.run_test(spec)))
                    else {
                        // The panic hook reports the cause. Stop using the engine after an unexpected panic.
                        let mut result =
                            TestResult::new(&spec.name).with_failure_reason("Flint test panicked");
                        result.minecraft_ids.clone_from(&spec.minecraft_ids);
                        results.push(result);
                        break;
                    };
                    results.push(result);
                }
                TestSpecLoadResult::Skipped { spec, reason } => {
                    results.push(TestResult::skipped(&spec.name, reason));
                }
            }
        }
        let summary = TestSummary::from_results(results);
        summary.print_concise_summary();
        save_summary(&summary)?;
        print_failure_urls(&specs, &paths, &summary, cfg);
        for result in &summary.results {
            if !result.skipped
                && let Some(reason) = &result.failure_reason
            {
                eprintln!("error: {}: {reason}", result.test_name);
            }
        }
        anyhow::ensure!(
            summary.total_tests > summary.skipped_tests,
            "No Flint tests executed"
        );
        anyhow::ensure!(summary.failed_tests == 0, "Flint tests failed");
        Ok(())
    }
}
