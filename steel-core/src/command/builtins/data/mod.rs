//! Vanilla data command.

mod block_accessor;
mod entity_accessor;
mod storage_accessor;

use super::super::{
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::command::brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError};
use simdnbt::owned::{NbtList, NbtTag};
use steel_utils::nbt::NbtPath;
use steel_utils::{Identifier, translations};

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;
type Ctx = SteelCommandContext<CommandSource>;

pub(super) const PATH_ARG: &str = "path";
pub(super) const SCALE_ARG: &str = "scale";

struct Accessor {
    get: fn() -> Builder,
    merge: fn() -> Builder,
    modify: fn() -> Builder,
    remove: fn() -> Builder,
}

const TARGET_ACCESSORS: [Accessor; 3] = [
    Accessor {
        get: block_accessor::get_target,
        merge: block_accessor::merge_target,
        modify: block_accessor::modify_target,
        remove: block_accessor::remove_target,
    },
    Accessor {
        get: entity_accessor::get_target,
        merge: entity_accessor::merge_target,
        modify: entity_accessor::modify_target,
        remove: entity_accessor::remove_target,
    },
    Accessor {
        get: storage_accessor::get_target,
        merge: storage_accessor::merge_target,
        modify: storage_accessor::modify_target,
        remove: storage_accessor::remove_target,
    },
];

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("data"), |_| command())
}

fn command() -> Builder {
    literal("data").then(
        TARGET_ACCESSORS
            .into_iter()
            .fold(literal("get"), |builder, accessor| {
                builder.then((accessor.get)())
            }),
    )
}

pub(super) fn path_scale_args(
    get_data_from_path: fn(context: &Ctx, arg: &str) -> Result<i32, CommandSyntaxError>,
    get_numeric_value: fn(context: &Ctx, arg: &str) -> Result<i32, CommandSyntaxError>,
    arg: String,
) -> Builder {
    argument(PATH_ARG, SteelArgumentType::nbt_path())
        .executes({
            let a = arg.clone();
            move |ctx| get_data_from_path(ctx, &a)
        })
        .then(
            argument(SCALE_ARG, ArgumentType::double(f64::MIN, f64::MAX)).executes({
                let a = arg.clone();
                move |ctx| get_numeric_value(ctx, &a)
            }),
        )
}

pub(super) fn process_numeric_arg(tag: NbtTag, scale: f64) -> Option<i32> {
    let val: f64 = match tag {
        NbtTag::Byte(b) => f64::from(b),
        NbtTag::Short(s) => f64::from(s),
        NbtTag::Int(i) => f64::from(i),
        NbtTag::Long(l) => l as f64,
        NbtTag::Float(f) => f64::from(f),
        NbtTag::Double(d) => d,
        _ => {
            return None;
        }
    };

    Some((val * scale).floor() as i32)
}

/// Takes a NbtTag and a NbtPath and returns a single Tag at that path.
/// Mirrors vanilla `DataCommands.getSingleTag`
// This is also used by the function command
pub(crate) fn get_single_tag(
    tag: &NbtTag,
    path: &NbtPath,
) -> Result<Option<NbtTag>, CommandSyntaxError> {
    let tags = path.get(tag);
    if tags.is_empty() {
        return Ok(None);
    }
    if tags.len() > 1 {
        return Err(CommandSyntaxError::dynamic(
            &translations::COMMANDS_DATA_GET_MULTIPLE,
        ));
    }

    Ok(Some(tags[0].clone()))
}

fn process_path_arg(tag: NbtTag) -> i32 {
    match tag.clone() {
        NbtTag::Byte(int) => i32::from(int),
        NbtTag::Short(int) => i32::from(int),
        NbtTag::Int(int) => int,
        NbtTag::Long(int) => int as i32,
        NbtTag::Float(float) => float.floor() as i32,
        NbtTag::Double(float) => float.floor() as i32,
        NbtTag::List(list) => match &list {
            NbtList::Empty => 0,
            NbtList::Byte(v) => v.len() as i32,
            NbtList::Short(v) => v.len() as i32,
            NbtList::Int(v) => v.len() as i32,
            NbtList::Long(v) => v.len() as i32,
            NbtList::Float(v) => v.len() as i32,
            NbtList::Double(v) => v.len() as i32,
            NbtList::ByteArray(v) => v.len() as i32,
            NbtList::String(v) => v.len() as i32,
            NbtList::List(v) => v.len() as i32,
            NbtList::Compound(v) => v.len() as i32,
            NbtList::IntArray(v) => v.len() as i32,
            NbtList::LongArray(v) => v.len() as i32,
        },
        NbtTag::ByteArray(arr) => arr.len() as i32,
        NbtTag::IntArray(arr) => arr.len() as i32,
        NbtTag::LongArray(arr) => arr.len() as i32,
        NbtTag::Compound(comp) => comp.len() as i32,
        NbtTag::String(str) => str.len() as i32,
    }
}
