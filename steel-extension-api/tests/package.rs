//! Native archive loading through the independently compiled boundary fixture.
#![cfg(feature = "host")]
use std::error::Error;

use std::{
    env::consts::{ARCH, DLL_PREFIX, DLL_SUFFIX, OS},
    fs::{self, File},
    io::Write,
    path::PathBuf,
};
use steel_core::{
    bootstrap::init_globals, extension::ExtensionHooks, worldgen::WorldGeneratorRegistry,
};
use steel_extension_api::host;
use zip::{ZipWriter, write::SimpleFileOptions};

#[test]
#[ignore = "requires cargo build --manifest-path steel-extension-api/tests/fixture/Cargo.toml"]
fn loads_a_packaged_native_extension_and_checks_its_manifest_identity() -> Result<(), Box<dyn Error>>
{
    let file = format!("{DLL_PREFIX}steel_behavior_boundary_fixture{DLL_SUFFIX}");
    let library = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixture/target/debug")
            .join(&file),
    )?;
    let directory = tempfile::tempdir()?;
    let mut hooks = ExtensionHooks::default();
    for (name, succeeds) in [
        ("incorrect-name", false),
        ("behavior-boundary-fixture", true),
    ] {
        let path = directory.path().join(format!("{name}.zip"));
        let mut zip = ZipWriter::new(File::create(&path)?);
        zip.start_file("extension.toml", SimpleFileOptions::default())?;
        write!(
            zip,
            "[package]\nname = '{name}'\nversion = '1'\n[targets]\n{OS}-{ARCH} = '{file}'\n"
        )?;
        zip.start_file(&file, SimpleFileOptions::default())?;
        zip.write_all(&library)?;
        zip.finish()?;
        // SAFETY: the archive contains the independently built trusted fixture;
        // load validates its descriptor and pins the extracted native library.
        let loaded = unsafe { host::load(&path, "", &mut hooks) };
        if !succeeds {
            assert!(
                loaded
                    .err()
                    .is_some_and(|error| error.contains("differs from its package manifest"))
            );
            continue;
        }
        let extension = loaded?;
        init_globals();
        let _generators = WorldGeneratorRegistry::new_with_builtins()?;
        // The fixture init also emits native filters and rejects an incompatible
        // payload, exercising both directions of the checked host boundary.
        assert_eq!(host::initialize_all(vec![extension], &mut hooks)?, [name]);
        // SAFETY: this is the same trusted archive; duplicate identities must fail.
        assert!(unsafe { host::load(&path, "", &mut hooks) }.is_err());
    }
    Ok(())
}
