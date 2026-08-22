#[cfg(test)]
mod tests {
    use crate::discovery::{DiscoveredMod, ModSource};
    use crate::manifest::ModManifest;
    use crate::resolver::{DependencyResolver, ResolutionError};
    use semver::Version;
    use std::path::Path;

    #[test]
    fn test_manifest_parsing() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "example_mod",
            "version": "1.2.3",
            "name": "Example Mod",
            "description": "An example mod",
            "depends": {
                "steel": ">=0.15.0"
            },
            "breaks": {
                "conflicting_mod": "*"
            },
            "entrypoints": {
                "main": ["example_mod_init"]
            }
        }"#;

        let manifest = ModManifest::parse_json(json).expect("failed to parse manifest");
        assert_eq!(manifest.id, "example_mod");
        assert_eq!(manifest.version, Version::parse("1.2.3").unwrap());
        assert_eq!(manifest.depends.len(), 1);
        assert_eq!(manifest.breaks.len(), 1);
        assert_eq!(
            manifest.entrypoints.get("main").unwrap(),
            &vec!["example_mod_init".to_string()]
        );
    }

    #[test]
    fn test_dependency_resolution() {
        let mod_a_json = r#"{
            "schemaVersion": 1,
            "id": "mod_a",
            "version": "1.0.0",
            "depends": {
                "mod_b": ">=1.0.0"
            }
        }"#;

        let mod_b_json = r#"{
            "schemaVersion": 1,
            "id": "mod_b",
            "version": "1.0.0"
        }"#;

        let mod_a = DiscoveredMod {
            manifest: ModManifest::parse_json(mod_a_json).unwrap(),
            source: ModSource::Directory(Path::new("/tmp/mod_a").to_path_buf()),
        };

        let mod_b = DiscoveredMod {
            manifest: ModManifest::parse_json(mod_b_json).unwrap(),
            source: ModSource::Directory(Path::new("/tmp/mod_b").to_path_buf()),
        };

        let resolved = DependencyResolver::resolve(vec![mod_a, mod_b]).unwrap();
        assert_eq!(resolved.len(), 2);
        // mod_b must come before mod_a because mod_a depends on mod_b
        assert_eq!(resolved[0].manifest.id, "mod_b");
        assert_eq!(resolved[1].manifest.id, "mod_a");
    }

    #[test]
    fn test_missing_dependency() {
        let mod_a_json = r#"{
            "schemaVersion": 1,
            "id": "mod_a",
            "version": "1.0.0",
            "depends": {
                "non_existent": ">=1.0.0"
            }
        }"#;

        let mod_a = DiscoveredMod {
            manifest: ModManifest::parse_json(mod_a_json).unwrap(),
            source: ModSource::Directory(Path::new("/tmp/mod_a").to_path_buf()),
        };

        let err = DependencyResolver::resolve(vec![mod_a]).unwrap_err();
        match err {
            ResolutionError::MissingDependency { dependency, .. } => {
                assert_eq!(dependency, "non_existent");
            }
            _ => panic!("Expected MissingDependency error"),
        }
    }
}
