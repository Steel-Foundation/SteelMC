//! Dependency resolution and topological sorting for mods.

use std::collections::{HashMap, HashSet};

use semver::Version;
use thiserror::Error;

use crate::discovery::DiscoveredMod;

/// Errors occurring during mod dependency resolution.
#[derive(Debug, Error)]
pub enum ResolutionError {
    /// Required dependency missing.
    #[error("Mod '{mod_id}' requires missing dependency '{dependency}' ({requirement})")]
    MissingDependency {
        /// Dependent mod id.
        mod_id: String,
        /// Required dependency id.
        dependency: String,
        /// Version requirement.
        requirement: String,
    },

    /// Dependency version mismatch.
    #[error(
        "Mod '{mod_id}' requires '{dependency}' version {requirement}, but found version {found_version}"
    )]
    VersionMismatch {
        /// Dependent mod id.
        mod_id: String,
        /// Dependency id.
        dependency: String,
        /// Requirement.
        requirement: String,
        /// Found version.
        found_version: Version,
    },

    /// Mod conflict (breaks).
    #[error(
        "Mod '{mod_id}' conflicts with installed mod '{conflicting_mod}' (version {found_version} satisfies break requirement {break_requirement})"
    )]
    Conflict {
        /// Mod defining break rule.
        mod_id: String,
        /// Conflicting mod id.
        conflicting_mod: String,
        /// Installed version of conflicting mod.
        found_version: Version,
        /// Break requirement rule.
        break_requirement: String,
    },

    /// Cyclic dependency detected.
    #[error("Cyclic dependency detected involving mods: {cycle:?}")]
    DependencyCycle {
        /// Cycle mod IDs.
        cycle: Vec<String>,
    },
}

/// Dependency resolver for mods.
pub struct DependencyResolver;

impl DependencyResolver {
    /// Resolves dependencies and returns a list of discovered mods in topological execution order.
    ///
    /// # Errors
    /// Returns `ResolutionError` if any missing dependencies, version mismatches, conflicts, or cycles exist.
    pub fn resolve(discovered: Vec<DiscoveredMod>) -> Result<Vec<DiscoveredMod>, ResolutionError> {
        let mut map: HashMap<String, DiscoveredMod> = HashMap::new();
        for m in discovered {
            map.insert(m.manifest.id.clone(), m);
        }

        // Validate dependencies and conflicts
        for m in map.values() {
            let mod_id = &m.manifest.id;

            // Check depends
            for (dep_id, req) in &m.manifest.depends {
                let Some(dep) = map.get(dep_id) else {
                    return Err(ResolutionError::MissingDependency {
                        mod_id: mod_id.clone(),
                        dependency: dep_id.clone(),
                        requirement: req.to_string(),
                    });
                };

                if !req.matches(&dep.manifest.version) {
                    return Err(ResolutionError::VersionMismatch {
                        mod_id: mod_id.clone(),
                        dependency: dep_id.clone(),
                        requirement: req.to_string(),
                        found_version: dep.manifest.version.clone(),
                    });
                }
            }

            // Check breaks
            for (break_id, req) in &m.manifest.breaks {
                if let Some(target) = map.get(break_id) {
                    if req.matches(&target.manifest.version) {
                        return Err(ResolutionError::Conflict {
                            mod_id: mod_id.clone(),
                            conflicting_mod: break_id.clone(),
                            found_version: target.manifest.version.clone(),
                            break_requirement: req.to_string(),
                        });
                    }
                }
            }
        }

        // Topological Sort (Kahn's algorithm / DFS)
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();
        let mut ordered_ids = Vec::new();

        fn dfs(
            node: &str,
            map: &HashMap<String, DiscoveredMod>,
            visited: &mut HashSet<String>,
            in_stack: &mut HashSet<String>,
            ordered: &mut Vec<String>,
        ) -> Result<(), ResolutionError> {
            if in_stack.contains(node) {
                return Err(ResolutionError::DependencyCycle {
                    cycle: vec![node.to_owned()],
                });
            }
            if !visited.contains(node) {
                visited.insert(node.to_owned());
                in_stack.insert(node.to_owned());

                if let Some(m) = map.get(node) {
                    for dep_id in m.manifest.depends.keys() {
                        if map.contains_key(dep_id) {
                            dfs(dep_id, map, visited, in_stack, ordered)?;
                        }
                    }
                }

                in_stack.remove(node);
                ordered.push(node.to_owned());
            }
            Ok(())
        }

        let mut all_ids: Vec<String> = map.keys().cloned().collect();
        all_ids.sort(); // Deterministic ordering

        for id in &all_ids {
            if !visited.contains(id) {
                dfs(id, &map, &mut visited, &mut in_stack, &mut ordered_ids)?;
            }
        }

        let mut result = Vec::new();
        for id in ordered_ids {
            if let Some(m) = map.remove(&id) {
                result.push(m);
            }
        }

        Ok(result)
    }
}
