//! Flint adapter implementation for `SteelMC`.
//!
//! This module provides the [`SteelAdapter`] which implements the `FlintAdapter` trait,
//! allowing the Flint testing framework to create test worlds using the real steel-core
//! World implementation.

use flint_core::{FlintAdapter, FlintWorld, ServerInfo};

use crate::world::SteelTestWorld;

/// Adapter for running Flint tests against `SteelMC`.
///
/// This adapter creates test worlds that use the real steel-core World
/// with RAM-only storage for instant chunk creation.
pub struct SteelAdapter {
    /// Server info for identification
    info: ServerInfo,
}

impl SteelAdapter {
    /// Creates a new Steel adapter.
    ///
    /// Note: You must call `steel_flint::init()` before creating an adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            info: ServerInfo {
                minecraft_version: "1.21.11".to_string(),
            },
        }
    }
}

impl Default for SteelAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FlintAdapter for SteelAdapter {
    fn create_test_world(&self) -> Box<dyn FlintWorld> {
        Box::new(SteelTestWorld::new())
    }

    fn server_info(&self) -> ServerInfo {
        self.info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_registries;
    use crate::{TestLoader, TestRunner};
    use dotenvy::dotenv;
    use flint_core::test_spec;
    use flint_core::utils::get_test_path;
    use std::env::var;
    use std::path::PathBuf;
    use test_spec::TestSpec;

    fn init_env() {
        dotenv().ok();
    }

    /// Collects test file paths based on environment variables.
    /// Priority: `FLINT_TEST` > `FLINT_PATTERN` > `FLINT_TAGS` > all
    fn collect_filtered_paths(loader: &TestLoader) -> Vec<PathBuf> {
        // Single test by name
        if let Ok(test_name) = var("FLINT_TEST") {
            println!("Running single test: {test_name}");
            return loader
                .collect_all_test_files()
                .unwrap_or_default()
                .into_iter()
                .filter(|p| {
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|name| name == test_name)
                })
                .collect();
        }

        // Pattern matching (glob-style)
        if let Ok(pattern) = var("FLINT_PATTERN") {
            println!("Running tests matching pattern: {pattern}");
            return loader
                .collect_all_test_files()
                .unwrap_or_default()
                .into_iter()
                .filter(|p| {
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|name| matches_pattern(name, &pattern))
                })
                .collect();
        }

        // Tag filtering
        if let Ok(tags_str) = var("FLINT_TAGS") {
            let tags: Vec<String> = tags_str.split(',').map(|s| s.trim().to_string()).collect();
            println!("Running tests with tags: {}", tags.join(", "));
            return loader.collect_by_tags(&tags).unwrap_or_default();
        }

        // Default: all tests
        println!("Running all flint tests");
        loader.collect_all_test_files().unwrap_or_default()
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
    fn generate_test_specs(paths: Vec<PathBuf>) -> Vec<TestSpec> {
        paths
            .iter()
            .filter_map(|path| {
                TestSpec::from_file(path)
                    .map_err(|e| println!("Failed to load {}: {}", path.display(), e))
                    .ok()
            })
            .collect()
    }

    #[test]
    fn test_run_flint_selected() {
        init_test_registries();
        init_env();

        // Load the fence test
        let test_path = PathBuf::from(get_test_path());
        let loader = TestLoader::new(&test_path, true)
            .unwrap_or_else(|e| panic!("error while loading test files: {e}"));
        let paths = collect_filtered_paths(&loader);
        let specs: Vec<TestSpec> = generate_test_specs(paths);

        // Create adapter and runner
        let adapter = SteelAdapter::new();
        let runner = TestRunner::new(&adapter);

        // Run the test
        let summary = runner.run_tests(&specs);
        summary.print_concise_summary();
        assert_eq!(summary.failed_tests, 0, "Not all flint tests passed!");
    }

    #[test]
    fn test_run_all_flint_benchmarks() {
        init_test_registries();
        init_env();

        let test_dir = PathBuf::from(get_test_path());
        if !test_dir.exists() {
            println!("FlintBenchmark tests directory not found, skipping");
            return;
        }

        let loader = TestLoader::new(&test_dir, true)
            .unwrap_or_else(|e| panic!("error while loading test files: {e}"));
        let paths = loader
            .collect_all_test_files()
            .unwrap_or_else(|e| panic!("error while loading test files: {e}"));

        if paths.is_empty() {
            println!("No test files matched the filter criteria");
            return;
        }

        println!("Found {} test(s) to run", paths.len());

        // Load all test specs from paths
        let specs: Vec<TestSpec> = generate_test_specs(paths);

        let adapter = SteelAdapter::new();
        let runner = TestRunner::new(&adapter);
        let summary = runner.run_tests(&specs);
        summary.print_test_summary(30);
        assert_eq!(summary.failed_tests, 0, "No tests were run");
    }
}
