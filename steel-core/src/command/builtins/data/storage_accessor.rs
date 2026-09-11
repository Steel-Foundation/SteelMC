use super::super::super::execution::{
    CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument, literal,
};
use crate::command::brigadier::{CommandNodeBuilder, CommandSyntaxError};
use crate::command::builtins::data::{
    PATH_ARG, SCALE_ARG, get_single_tag, path_scale_args, process_numeric_arg, process_path_arg,
};
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::nbt::NbtPath;
use steel_utils::text::command_nbt_component;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const ACCESSOR_KEYWORD: &str = "storage";
const TARGET_ARG: &str = "target";
const SOURCE_ARG: &str = "source";

pub(super) fn get_target() -> Builder {
    literal(ACCESSOR_KEYWORD).then(
        argument(TARGET_ARG, SteelArgumentType::storage_key())
            .executes(|ctx| get_data(ctx, TARGET_ARG.to_string()))
            .then(path_scale_args(
                get_tag_from_path,
                get_numeric_value,
                TARGET_ARG.to_string(),
            )),
    )
}

pub(super) fn get_source() -> Builder {
    literal(ACCESSOR_KEYWORD).then(
        argument(SOURCE_ARG, SteelArgumentType::storage_key())
            .executes(|ctx| get_data(ctx, SOURCE_ARG.to_string()))
            .then(path_scale_args(
                get_tag_from_path,
                get_numeric_value,
                SOURCE_ARG.to_string(),
            )),
    )
}

fn get_data(
    context: &SteelCommandContext<CommandSource>,
    arg: String,
) -> Result<i32, CommandSyntaxError> {
    let id = context.identifier(&arg)?;
    let domain = context.source().world().domain();

    let tag =
        if let Some(command_storage) = context.source().server().command_storage.get(domain) {
            command_storage.get(id)
        } else {
            unreachable!()
        }
        .to_nbt_tag();

    context
        .source()
        .send_success(&print_success(&tag, &id), false);

    Ok(1)
}

fn get_tag_from_path(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<i32, CommandSyntaxError> {
    let (tag, id, ..) = get_single(context, arg)?;

    context
        .source()
        .send_success(&print_success(&tag, &id), false);

    Ok(process_path_arg(tag))
}

fn get_single(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<(NbtTag, Identifier, NbtPath), CommandSyntaxError> {
    let id = context.identifier(&arg)?.clone();
    let domain = context.source().world().domain();
    let path = context.nbt_path(PATH_ARG)?.clone();

    let tag =
        if let Some(command_storage) = context.source().server().command_storage.get(domain) {
            command_storage.get(&id)
        } else {
            // TODO Is this unreachable??
            unreachable!()
        }
        .to_nbt_tag();

    let s_tag = if let Some(t) = get_single_tag(&tag, &path)? {
        t
    } else {
        return Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_DATA_GET_UNKNOWN
                .message([TextComponent::plain((&path).as_str().to_string())])
                .component(),
        ));
    };

    Ok((s_tag, id, path))
}

fn get_numeric_value(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<i32, CommandSyntaxError> {
    let (tag, id, path) = get_single(context, arg)?;
    let scale = context.double(SCALE_ARG)?;

    if let Some(result) = process_numeric_arg(tag, scale) {
        context
            .source()
            .send_success(&print_success_scaled(&path, &id, scale, result), false);

        Ok(result)
    } else {
        Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_DATA_GET_INVALID
                .message([TextComponent::plain(path.as_str().to_string())]),
        ))
    }
}

fn print_success(data: &NbtTag, id: &Identifier) -> TextComponent {
    translations::COMMANDS_DATA_STORAGE_QUERY
        .message([
            TextComponent::plain(id.to_string()),
            command_nbt_component(data, false),
        ])
        .component()
}

fn print_success_scaled(path: &NbtPath, id: &Identifier, scale: f64, val: i32) -> TextComponent {
    translations::COMMANDS_DATA_ENTITY_GET
        .message([
            TextComponent::plain(path.as_str().to_string()),
            TextComponent::plain(id.to_string()),
            TextComponent::plain(format!("{scale:.2}")),
            TextComponent::plain(val.to_string()),
        ])
        .component()
}

pub(super) fn merge_target() -> Builder {
    literal(ACCESSOR_KEYWORD)
}
pub(super) fn merge_source() -> Builder {
    literal(ACCESSOR_KEYWORD)
}

pub(super) fn modify_target() -> Builder {
    todo!()
}
pub(super) fn modify_source() -> Builder {
    todo!()
}

pub(super) fn remove_target() -> Builder {
    todo!()
}
pub(super) fn remove_source() -> Builder {
    todo!()
}
