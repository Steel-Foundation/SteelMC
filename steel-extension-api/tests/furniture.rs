//! Native furniture registration and real placement, mounting, and sleeping.
#![cfg(feature = "host")]

#[path = "furniture/bed.rs"]
mod bed;
#[path = "furniture/chair.rs"]
mod chair;
#[path = "lucky_block/fixture.rs"]
mod fixture;
#[path = "furniture/helpers.rs"]
mod helpers;
#[path = "furniture/placement.rs"]
mod placement;
#[path = "furniture/slabs.rs"]
mod slabs;

use std::{
    env::{
        consts::{DLL_PREFIX, DLL_SUFFIX},
        var_os,
    },
    error::Error,
    path::PathBuf,
};
use steel_core::{bootstrap::init_globals, extension::ExtensionHooks};
use steel_extension_api::host;
use steel_registry::vanilla_dimension_types;

#[test]
#[ignore = "requires building extension-example/furniture"]
fn furniture_places_sits_sleeps_breaks_and_persists() -> Result<(), Box<dyn Error>> {
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-example/furniture/target/debug")
        .join(format!("{DLL_PREFIX}steel_furniture_extension{DLL_SUFFIX}"));
    let mut hooks = ExtensionHooks::default();
    let pack = var_os("STEEL_TEST_FURNITURE_PACK").is_some();
    let config = if pack { "use_resource_pack = true" } else { "" };
    // SAFETY: this trusted example uses the checked native v3 contract; the host
    // pins its library and all callback allocations for their required lifetime.
    let extension = unsafe { host::load(&library, config, &mut hooks) }?;
    init_globals();
    let data_root = tempfile::tempdir()?;
    let _initialized = host::initialize_all(vec![extension], data_root.path(), &mut hooks)?;
    let mut fixture = fixture::Fixture::with_dimension(&vanilla_dimension_types::OVERWORLD, true)?;
    helpers::floor(&fixture);
    fixture.join_player()?;
    placement::check()?;
    chair::check(&fixture)?;
    slabs::check(&fixture, pack)?;
    bed::check(&fixture)?;
    bed::check_explosion()?;
    Ok(())
}
