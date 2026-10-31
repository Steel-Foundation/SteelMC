//! Independently compiled fixture covering complete behavior and entity delegation.
#![recursion_limit = "512"]
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use steel_extension_api::{
    ApiResult, BlockPos, BlockState, Hooks, Registry,
    block_behavior::{
        BlockBehavior, OriginalBlockBehavior,
        block_entity::BlockEntityBehavior,
        capabilities::{RailBehavior, RegisteredRailBehavior},
        types::{
            BlockEntity, BlockEntityCreation, BonemealableRef, BorrowedNbt, NbtMut, RailRef,
            WeakWorld,
        },
    },
    stabby::{self, boxed::Box, option::Option as StableOption, string::String as ApiString},
};

struct Inherit(OriginalBlockBehavior);
#[stabby::export(callback)]
impl BlockBehavior for Inherit {
    fn original(&self) -> OriginalBlockBehavior {
        self.0
    }
}
struct DisableBonemeal(OriginalBlockBehavior);
#[stabby::export(callback)]
impl BlockBehavior for DisableBonemeal {
    fn original(&self) -> OriginalBlockBehavior {
        self.0
    }
    fn as_bonemealable(&self) -> StableOption<BonemealableRef> {
        StableOption::None()
    }
}
struct StraightRail;
#[stabby::export(callback)]
impl RailBehavior for StraightRail {
    fn is_straight(&self) -> bool {
        true
    }
}
struct Rail {
    original: OriginalBlockBehavior,
    rail: RegisteredRailBehavior,
}
#[stabby::export(callback)]
impl BlockBehavior for Rail {
    fn original(&self) -> OriginalBlockBehavior {
        self.original
    }
    fn as_rail(&self) -> StableOption<RailRef> {
        Some(self.rail.borrow()).into()
    }
}
struct CounterBlock(OriginalBlockBehavior);
#[stabby::export(callback)]
impl BlockBehavior for CounterBlock {
    fn original(&self) -> OriginalBlockBehavior {
        self.0
    }
    fn new_block_entity(
        &self,
        level: WeakWorld,
        pos: BlockPos,
        state: BlockState,
    ) -> BlockEntityCreation {
        let original = self
            .0
            .new_block_entity(level, pos, state)
            .into_created()
            .match_owned(
                |entity| entity,
                || panic!("comparator fixture requires its original entity"),
            );
        let data = Box::new(Counter {
            original: original.share(),
            count: AtomicI32::new(0),
        })
        .into();
        BlockEntityCreation::created(original.with_behavior(data))
    }
}
struct Counter {
    original: BlockEntity,
    count: AtomicI32,
}
#[stabby::export(callback)]
impl BlockEntityBehavior for Counter {
    fn load_additional(&self, nbt: BorrowedNbt<'_>) {
        if let Some(count) = nbt.int("extension_count".into()).as_ref() {
            self.count.store(*count, Ordering::Relaxed);
        }
        self.original.borrow().load_additional(nbt);
    }
    fn save_additional(&self, mut nbt: NbtMut<'_>) {
        nbt.put_int("extension_count".into(), self.count.load(Ordering::Relaxed));
        self.original.borrow().save_additional(nbt);
    }
    fn trigger_event(&self, kind: i32, value: i32) -> bool {
        if kind == 1234 {
            self.count.store(value, Ordering::Relaxed);
            return true;
        }
        self.original.borrow().trigger_event(kind, value)
    }
}

#[stabby::export(callback)]
fn register_blocks(mut registry: Registry<'_>) -> ApiResult<()> {
    let result = (|| -> Result<(), ApiString> {
        let carrots = OriginalBlockBehavior::get(&registry, "minecraft:carrots".into())
            .match_owned(Ok, Err)?;
        if carrots.as_bonemealable().is_none() {
            return Err("original carrot capability is unavailable during registration".into());
        }
        registry
            .replace_block_behavior(
                "minecraft:carrots".into(),
                Box::new(Inherit(carrots)).into(),
            )
            .match_owned(Ok, Err)?;
        if registry.config().as_str() == "fail_after_replacement" {
            return Err("fixture failed after replacing carrots".into());
        }
        let potatoes = OriginalBlockBehavior::get(&registry, "minecraft:potatoes".into())
            .match_owned(Ok, Err)?;
        registry
            .replace_block_behavior(
                "minecraft:potatoes".into(),
                Box::new(DisableBonemeal(potatoes)).into(),
            )
            .match_owned(Ok, Err)?;
        let rail =
            OriginalBlockBehavior::get(&registry, "minecraft:rail".into()).match_owned(Ok, Err)?;
        let implementation = registry.create_rail(Box::new(StraightRail).into());
        registry
            .replace_block_behavior(
                "minecraft:rail".into(),
                Box::new(Rail {
                    original: rail,
                    rail: implementation,
                })
                .into(),
            )
            .match_owned(Ok, Err)?;
        let comparator = OriginalBlockBehavior::get(&registry, "minecraft:comparator".into())
            .match_owned(Ok, Err)?;
        registry
            .replace_block_behavior(
                "minecraft:comparator".into(),
                Box::new(CounterBlock(comparator)).into(),
            )
            .match_owned(Ok, Err)?;
        Ok(())
    })();
    result.into()
}
#[stabby::stabby]
struct OrderedNumber {
    value: u32,
}
impl steel_extension_api::hooks::Filter for OrderedNumber {
    const TAG: &'static str = "fixture:ordered_number/v1";
}
#[stabby::stabby]
struct IncompatibleNumber {
    value: u64,
}
#[stabby::stabby]
struct FixtureAction {
    value: u32,
}
impl steel_extension_api::hooks::Action for FixtureAction {
    const TAG: &'static str = "fixture:action/v1";
}
static ACTION_RESULT: AtomicU32 = AtomicU32::new(0);
#[stabby::export(callback)]
fn nested_action(action: &FixtureAction) {
    let mut number = OrderedNumber {
        value: action.value,
    };
    if steel_extension_api::hooks::apply_filters(&mut number).match_owned(|()| true, |_| false) {
        ACTION_RESULT.store(number.value, Ordering::Relaxed);
    }
}
impl steel_extension_api::hooks::Filter for IncompatibleNumber {
    const TAG: &'static str = "fixture:ordered_number/v1";
}
#[stabby::export(callback)]
fn first(number: &mut OrderedNumber) {
    number.value = number.value * 10 + 1;
}
#[stabby::export(callback)]
fn second(number: &mut OrderedNumber) {
    number.value = number.value * 10 + 2;
}
fn pre_init(mut hooks: Hooks<'_>) -> ApiResult<()> {
    let result = (|| -> Result<(), ApiString> {
        hooks
            .register_block_registration(register_blocks)
            .match_owned(Ok, Err)?;
        hooks
            .add_filter::<OrderedNumber>(first)
            .before("absent-extension")
            .register()
            .match_owned(Ok, Err)?;
        hooks
            .add_filter::<OrderedNumber>(second)
            .register()
            .match_owned(Ok, Err)?;
        hooks
            .add_action::<FixtureAction>(nested_action)
            .register()
            .match_owned(Ok, Err)?;
        Ok(())
    })();
    result.into()
}
fn init(config: stabby::str::Str<'_>) -> ApiResult<()> {
    if config.as_str() == "fail_after_replacement" {
        return Err(ApiString::from("init ran after failed registration")).into();
    }
    let result = (|| -> Result<(), ApiString> {
        let mut number = OrderedNumber { value: 0 };
        steel_extension_api::hooks::apply_filters(&mut number).match_owned(Ok, Err)?;
        if number.value != 12 {
            return Err("native filter chain ran out of order".into());
        }
        steel_extension_api::hooks::do_action(&FixtureAction { value: 1 }).match_owned(Ok, Err)?;
        if ACTION_RESULT.load(Ordering::Relaxed) != 112 {
            return Err("nested native action/filter dispatch failed".into());
        }
        let mut incompatible = IncompatibleNumber { value: 7 };
        if steel_extension_api::hooks::apply_filters(&mut incompatible)
            .match_owned(|()| true, |_| false)
        {
            return Err("native hook accepted an incompatible payload ABI".into());
        }
        steel_extension_api::default_block_state("minecraft:stone".into()).match_owned(Ok, Err)?;
        Ok(())
    })();
    result.into()
}
steel_extension_api::export_extension!("behavior-boundary-fixture", pre_init, init);
