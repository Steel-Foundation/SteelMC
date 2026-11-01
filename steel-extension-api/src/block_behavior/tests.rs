use super::{
    BlockBehavior, OriginalBlockBehavior,
    adapter::Adapter,
    capabilities::{RailBehavior, RegisteredRailBehavior},
    core_types,
    types::{BlockEntity, BonemealableRef, BorrowedNbt, NbtMut, RailRef, WeakWorld},
};
use crate::{BlockPos, BlockState, blocks_in_tag};
use crate::{Registry, host::Registration};
use stabby::boxed::Box as StableBox;
use stabby::{self, option::Option as StableOption};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Weak,
        atomic::{AtomicI32, AtomicUsize, Ordering},
    },
    thread,
};
use steel_core::{
    behavior::{
        BLOCK_BEHAVIORS, BlockBehavior as CoreBehavior, BlockBehaviorRegistry,
        block_behaviors::register_block_behaviors,
    },
    bootstrap::init_globals,
};
use steel_registry::{REGISTRY, RegistryExt, blocks::BlockRef, vanilla_blocks, vanilla_items};

#[test]
fn item_creation_validates_registry_keys_and_extracted_stack_limits() -> Result<(), String> {
    init_globals();
    let create = |key: &str, count| {
        super::types::ItemStack::from_key(key.into(), count)
            .match_owned(Ok, Err)
            .map_err(|error| error.to_string())
    };
    let stack = create("minecraft:diamond", 3)?.into_host();
    assert!(stack.is(&vanilla_items::DIAMOND));
    assert_eq!(stack.count(), 3);
    assert!(create("minecraft:diamond", 0).is_err());
    assert!(create("minecraft:diamond", -1).is_err());
    assert!(create("minecraft:diamond", 65).is_err());
    assert!(create("minecraft:diamond_sword", 2).is_err());
    assert!(create("minecraft:air", 1).is_err());
    assert!(create("lucky_test:missing_item", 1).is_err());
    assert!(create("invalid key", 1).is_err());
    Ok(())
}

#[test]
fn block_item_stacks_preserve_each_wool_color() -> Result<(), String> {
    init_globals();
    let wool = blocks_in_tag("minecraft:wool".into())
        .match_owned(Ok, Err)
        .map_err(|error| error.to_string())?;
    assert!(!wool.is_empty());
    for key in &wool {
        let key = key.as_str().parse().map_err(str::to_owned)?;
        let block = REGISTRY.blocks.by_key(&key).ok_or("missing wool block")?;
        let item = REGISTRY.items.by_key(&key).ok_or("missing wool item")?;
        let stack = super::types::ItemStack::from_block(BlockState {
            id: block.default_state().0,
        })
        .match_owned(Ok, Err)
        .map_err(|error| error.to_string())?
        .into_host();
        assert_eq!(stack.item(), item, "wool color changed for {key}");
        assert_eq!(stack.count(), 1);
    }
    let empty = super::types::ItemStack::from_block(BlockState {
        id: vanilla_blocks::AIR.default_state().0,
    })
    .match_owned(Ok, Err)
    .map_err(|error| error.to_string())?
    .into_host();
    assert!(empty.is_empty());
    Ok(())
}

fn original(block: BlockRef) -> OriginalBlockBehavior {
    OriginalBlockBehavior::from_host(Box::leak(Box::new(
        BLOCK_BEHAVIORS.get_behavior_shared(block),
    )))
}

struct Delegating(OriginalBlockBehavior);
#[stabby::export(callback)]
impl BlockBehavior for Delegating {
    fn original(&self) -> OriginalBlockBehavior {
        self.0
    }
    fn is_bed(&self) -> bool {
        true
    }
}

struct NoBonemeal(OriginalBlockBehavior);
#[stabby::export(callback)]
impl BlockBehavior for NoBonemeal {
    fn original(&self) -> OriginalBlockBehavior {
        self.0
    }
    fn as_bonemealable(&self) -> StableOption<BonemealableRef> {
        StableOption::None()
    }
}

#[test]
fn captures_keep_the_selected_behavior_after_multiple_registry_edits() -> Result<(), String> {
    init_globals();
    let mut behaviors = BlockBehaviorRegistry::new();
    register_block_behaviors(&mut behaviors);
    let (native, stripped, replacement) = {
        let mut registration = Registration {
            behaviors: &mut behaviors,
            config: "",
        };
        let mut registry = Registry::from_host(&mut registration);
        let key = "minecraft:carrots";
        let native = OriginalBlockBehavior::get(&registry, key.into())
            .match_owned(Ok, Err)
            .map_err(|e| e.to_string())?;
        registry
            .replace_block_behavior(key.into(), StableBox::new(NoBonemeal(native)).into())
            .match_owned(Ok, Err)
            .map_err(|e| e.to_string())?;
        let stripped = OriginalBlockBehavior::get(&registry, key.into())
            .match_owned(Ok, Err)
            .map_err(|e| e.to_string())?;
        registry
            .replace_block_behavior(key.into(), StableBox::new(Delegating(native)).into())
            .match_owned(Ok, Err)
            .map_err(|e| e.to_string())?;
        let replacement = OriginalBlockBehavior::get(&registry, key.into())
            .match_owned(Ok, Err)
            .map_err(|e| e.to_string())?;
        (native, stripped, replacement)
    };
    drop(behaviors);
    assert!(!native.is_bed());
    assert!(!stripped.is_bed());
    assert!(replacement.is_bed());
    assert!(native.as_bonemealable().is_some());
    assert!(stripped.as_bonemealable().is_none());
    assert!(replacement.as_bonemealable().is_some());
    assert!(
        thread::spawn(move || replacement.is_bed())
            .join()
            .expect("captured behavior call")
    );
    Ok(())
}

#[test]
fn omitted_methods_delegate_but_extensions_can_replace_or_remove_capabilities() {
    init_globals();
    let base = original(&vanilla_blocks::CARROTS);
    let native = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::CARROTS);
    assert!(native.as_bonemealable().is_some());
    let adapter = Adapter(StableBox::new(Delegating(base)).into());
    assert!(CoreBehavior::is_bed(&adapter));
    assert!(!base.is_bed());
    assert_eq!(
        CoreBehavior::fits_inside_container_items(&adapter),
        native.fits_inside_container_items()
    );
    assert!(CoreBehavior::as_bonemealable(&adapter).is_some());
    let stripped = Adapter(StableBox::new(NoBonemeal(base)).into());
    assert!(CoreBehavior::as_bonemealable(&stripped).is_none());
    assert!(base.as_bonemealable().is_some());
}

struct CustomRail;
#[stabby::export(callback)]
impl RailBehavior for CustomRail {
    fn is_straight(&self) -> bool {
        false
    }
}
struct WithRail {
    original: OriginalBlockBehavior,
    rail: RegisteredRailBehavior,
}
#[stabby::export(callback)]
impl BlockBehavior for WithRail {
    fn original(&self) -> OriginalBlockBehavior {
        self.original
    }
    fn as_rail(&self) -> StableOption<RailRef> {
        Some(self.rail.borrow()).into()
    }
    fn is_rail(&self) -> bool {
        true
    }
}

#[test]
fn registered_extension_capability_is_used_instead_of_the_original() {
    init_globals();
    let mut behaviors = BlockBehaviorRegistry::new();
    let mut registration = Registration {
        behaviors: &mut behaviors,
        config: "",
    };
    let mut registry = Registry::from_host(&mut registration);
    let rail = registry.create_rail(StableBox::new(CustomRail).into());
    let base = original(&vanilla_blocks::WHITE_WOOL);
    assert!(base.as_rail().is_none());
    let adapter = Adapter(
        StableBox::new(WithRail {
            original: base,
            rail,
        })
        .into(),
    );
    assert!(CoreBehavior::is_rail(&adapter));
    assert_eq!(
        CoreBehavior::as_rail(&adapter).map(core_types::RailBehavior::is_straight),
        Some(false)
    );
}

use super::block_entity::BlockEntityBehavior;
struct Counter {
    value: AtomicI32,
    drops: Arc<AtomicUsize>,
}
impl Drop for Counter {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}
#[stabby::export(callback)]
impl BlockEntityBehavior for Counter {
    fn load_additional<'a>(&self, nbt: BorrowedNbt<'a>) {
        if let Some(value) = nbt.int("count".into()).as_ref() {
            self.value.store(*value, Ordering::Relaxed);
        }
    }
    fn save_additional<'a>(&self, mut nbt: NbtMut<'a>) {
        nbt.put_int("count".into(), self.value.load(Ordering::Relaxed));
    }
}

#[test]
fn host_owned_entities_serialize_and_free_extension_data_exactly_once() -> Result<(), String> {
    init_globals();
    let drops = Arc::new(AtomicUsize::new(0));
    let entity = BlockEntity::new(
        "minecraft:comparator".into(),
        WeakWorld::from_host(Weak::new()),
        BlockPos { x: 0, y: 64, z: 0 },
        BlockState {
            id: vanilla_blocks::COMPARATOR.default_state().0,
        },
        StableBox::new(Counter {
            value: AtomicI32::new(7),
            drops: Arc::clone(&drops),
        })
        .into(),
    )
    .match_owned(Ok, Err)
    .map_err(|e| e.to_string())?;
    let shared = entity.share();
    let native = entity.into_host();
    let mut nbt = core_types::NbtCompound::new();
    native.save_additional(&mut nbt);
    assert_eq!(nbt.int("count"), Some(7));
    let mut updated = core_types::NbtCompound::new();
    updated.insert("count", 19_i32);
    native
        .load_with_owned_components(&updated)
        .map_err(|e| e.to_string())?;
    let mut saved = core_types::NbtCompound::new();
    saved.insert("count", 99_i32);
    native.save_additional(&mut saved);
    assert_eq!(saved.int("count"), Some(19));
    assert_eq!(
        saved.keys().filter(|key| key.to_str() == "count").count(),
        1
    );
    drop(native);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(shared);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    Ok(())
}

fn method_names(source: &str, prefix: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix(prefix))
        .filter_map(|line| line.split(['(', '<']).next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_core_block_method_has_an_extension_method_original_call_and_adapter() {
    let native = include_str!("../../../steel-core/src/behavior/block/mod.rs");
    let start = native.find("pub trait BlockBehavior").expect("core trait");
    let end = native[start..]
        .find("\nmod registry;")
        .expect("core trait end")
        + start;
    let mut expected = method_names(&native[start..end], "fn ");
    expected.remove("type_name");
    let api = method_names(include_str!("schema.rs"), "fn ");
    assert_eq!(
        api, expected,
        "new default core methods must also cross the extension boundary"
    );
}
