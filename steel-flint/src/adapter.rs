//! Flint adapter implementation for `SteelMC`.
//!
//! This module provides the [`SteelAdapter`] which implements the `FlintAdapter` trait,
//! allowing the Flint testing framework to create test worlds using the real steel-core
//! World implementation.

use crate::world::SteelTestWorld;
use dotenvy::dotenv;
use flint_core::results::TestSummary;
use flint_core::test_spec::MinimalTestSpec;
use flint_core::{FlintAdapter, FlintWorld, ServerInfo, TestLoader};
use rustc_hash::FxHashSet;
use serde::Deserialize;
use std::env::var;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use steel_core::behavior::block_behaviors::PLAIN_BLOCKS;
use steel_core::behavior::{BLOCK_BEHAVIORS, ITEM_BEHAVIORS};
use steel_core::entity::ENTITIES;
use steel_registry::{REGISTRY, RegistryExt};
use toml::from_str;

/// Adapter for running Flint tests against `SteelMC`.
///
/// Call `steel_flint::init()` before creating test worlds.
#[derive(Clone, Default)]
pub struct SteelAdapter;

impl FlintAdapter for SteelAdapter {
    fn create_test_world(&self) -> Result<Box<dyn FlintWorld + 'static>, anyhow::Error> {
        Ok(Box::new(SteelTestWorld::new()))
    }

    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            minecraft_version: steel_utils::MC_VERSION.to_string(),
        }
    }
}

/// Returns IDs of blocks with a non-default behavior or a plain vanilla `Block` class.
pub fn get_implemented_blocks() -> Vec<String> {
    BLOCK_BEHAVIORS
        .get_behaviors()
        .iter()
        .enumerate()
        .filter(|(_, behavior)| !behavior.type_name().ends_with("DefaultBlockBehavior"))
        .filter_map(|(id, _)| REGISTRY.blocks.by_id(id))
        .chain(PLAIN_BLOCKS.iter().copied())
        .map(|block| block.key.to_string())
        .collect()
}

/// Returns IDs of items with a non-default, non-block-item behavior.
pub fn get_implemented_items() -> Vec<String> {
    ITEM_BEHAVIORS
        .get_behaviors()
        .iter()
        .enumerate()
        .filter(|(_, behavior)| {
            let name = behavior.type_name();
            !name.ends_with("DefaultItemBehavior") && !name.ends_with("BlockItem")
        })
        .filter_map(|(id, _)| REGISTRY.items.by_id(id))
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

/// Loads `flint.toml` from the working directory, falling back to defaults if it is missing or invalid.
#[must_use]
pub fn load_config() -> FlintConfig {
    fs::read_to_string("flint.toml")
        .ok()
        .and_then(|s| from_str(&s).ok())
        .unwrap_or_default()
}

/// Loads environment variables from `.env`, if present.
pub fn init_env() {
    dotenv().ok();
}

/// Collects test file paths based on environment variables and `flint.toml`.
/// Priority: `FLINT_TEST` > `FLINT_TAGS` > `implemented_only` (`FLINT_IMPLEMENTED_ONLY` env > toml) > `filter.tags` > `FLINT_PATTERN` > all
#[must_use]
pub fn collect_filtered_paths(loader: &TestLoader, cfg: &FlintConfig) -> Vec<PathBuf> {
    let filter = cfg.filter.as_ref();

    // Single test by name: env > toml
    let test_name = var("FLINT_TEST")
        .ok()
        .or_else(|| filter.and_then(|f| f.test.clone()));
    if let Some(name) = test_name {
        println!("Running single test: {name}");
        return loader
            .collect_all_test_files()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n == name)
            })
            .collect();
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
        let implemented: FxHashSet<String> = ids.iter().cloned().collect();
        let paths: Vec<PathBuf> = loader
            .collect_by_tags(&ids)
            .into_iter()
            .filter(|path| all_ids_implemented(path, &implemented))
            .collect();
        println!(
            "Running {} test(s) whose minecraftIds are all implemented",
            paths.len()
        );
        return paths;
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
        return loader.collect_by_tags(&tags);
    }

    // Pattern matching: env > toml
    let pattern = var("FLINT_PATTERN")
        .ok()
        .or_else(|| filter.and_then(|f| f.pattern.clone()));
    if let Some(p) = pattern {
        println!("Running tests matching pattern: {p}");
        return loader
            .collect_all_test_files()
            .unwrap_or_default()
            .into_iter()
            .filter(|path| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|name| matches_pattern(name, &p))
            })
            .collect();
    }

    // Default: all tests
    println!("Running all flint tests");
    loader.collect_all_test_files().unwrap_or_default()
}

/// Whether every `minecraftIds` entry of the test at `path` is implemented.
/// Unreadable specs are kept so `load_specs` reports the real error.
fn all_ids_implemented(path: &Path, implemented: &FxHashSet<String>) -> bool {
    let Some(spec) = fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str::<MinimalTestSpec>(&json).ok())
    else {
        return true;
    };
    !spec.minecraft_ids.is_empty() && spec.minecraft_ids.iter().all(|id| implemented.contains(id))
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
    fs::write("log/flint_summary.json", summary.create_ci_output(true))?;
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
    // specs/paths/results run in lockstep — `run_tests` preserves order.
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

    #[test]
    #[ignore = "needs FlintBenchmark tests (TEST_PATH); run with --ignored"]
    fn test_run_flint_selected() {
        crate::init();
        init_env();

        let cfg = load_config();
        let loader = TestLoader::new(&cfg.test_path(), true)
            .unwrap_or_else(|e| panic!("error while loading test files: {e}"));
        let paths = collect_filtered_paths(&loader, &cfg);
        let specs = loader
            .load_specs(&paths, false)
            .unwrap_or_else(|e| panic!("error while loading test specs: {e}"));

        let runner = TestRunner::new(Arc::new(SteelAdapter));
        let summary = runner.run_tests(&specs);
        summary.print_concise_summary();
        save_summary(&summary).unwrap_or_else(|e| panic!("failed to save flint summary: {e}"));
        print_failure_urls(&specs, &paths, &summary, &cfg);
        assert_eq!(summary.failed_tests, 0, "Not all flint tests passed!");
    }

    #[test]
    #[ignore = "needs FlintBenchmark tests (TEST_PATH); run with --ignored"]
    fn test_run_all_flint_benchmarks() {
        crate::init();
        init_env();

        let cfg = load_config();
        let test_dir = cfg.test_path();
        assert!(
            test_dir.exists(),
            "FlintBenchmark tests directory not found, skipping"
        );

        let loader = TestLoader::new(&test_dir, true)
            .unwrap_or_else(|e| panic!("error while loading test files: {e}"));
        let paths = loader
            .collect_all_test_files()
            .unwrap_or_else(|e| panic!("error while loading test files: {e}"));
        assert!(
            !paths.is_empty(),
            "No test files matched the filter criteria in {}",
            test_dir.to_str().unwrap_or("NONE")
        );

        println!("Found {} test(s) to run", paths.len());

        let specs = loader
            .load_specs(&paths, false)
            .unwrap_or_else(|e| panic!("error while loading test specs: {e}"));

        let runner = TestRunner::new(Arc::new(SteelAdapter));
        let summary = runner.run_tests(&specs);
        summary.print_concise_summary();
        save_summary(&summary).unwrap_or_else(|e| panic!("failed to save flint summary: {e}"));
        print_failure_urls(&specs, &paths, &summary, &cfg);
        assert_eq!(summary.failed_tests, 0, "No tests were run");
    }
}
