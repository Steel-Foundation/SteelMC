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
use steel_utils::{BlockPos, translations};
use text_components::TextComponent;

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const ACCESSOR_KEYWORD: &str = "block";
const ARG: &str = "Pos";
const TARGET_PREFIX: &str = "target";
const SOURCE_PREFIX: &str = "source";

pub(super) fn get_target() -> Builder {
    let arg = format!("{TARGET_PREFIX}{ARG}");
    literal(ACCESSOR_KEYWORD).then(
        argument(arg.clone(), SteelArgumentType::block_pos())
            .executes({
                let a = arg.clone();
                move |ctx| get_data(ctx, a.clone())
            })
            .then(path_scale_args(get_tag_from_path, get_numeric_value, arg)),
    )
}

pub(super) fn get_source() -> Builder {
    let arg = format!("{SOURCE_PREFIX}{ARG}");
    literal(ACCESSOR_KEYWORD).then(
        argument(arg.clone(), SteelArgumentType::block_pos())
            .executes({
                let a = arg.clone();
                move |ctx| get_data(ctx, a.clone())
            })
            .then(path_scale_args(get_tag_from_path, get_numeric_value, arg)),
    )
}

fn get_data(
    context: &SteelCommandContext<CommandSource>,
    arg: String,
) -> Result<i32, CommandSyntaxError> {
    let coordinates = context.coordinates(&arg)?;
    let block_pos = coordinates.block_pos(context.source());

    if let Some(entity) = context.source().world().get_block_entity(block_pos) {
        let tag = entity.save_with_full_metadata().to_nbt_tag();

        context
            .source()
            .send_success(&print_success(&tag, &block_pos), false);
        return Ok(1);
    }

    Err(CommandSyntaxError::dynamic(TextComponent::from(
        &translations::COMMANDS_DATA_BLOCK_INVALID,
    )))
}

fn get_tag_from_path(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<i32, CommandSyntaxError> {
    let (tag, block_pos, ..) = get_single(context, arg)?;

    context
        .source()
        .send_success(&print_success(&tag, &block_pos), false);

    Ok(process_path_arg(tag))
}

fn get_single(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<(NbtTag, BlockPos, NbtPath), CommandSyntaxError> {
    let coordinates = context.coordinates(arg)?;
    let block_pos = coordinates.block_pos(context.source());
    let path = context.nbt_path(PATH_ARG)?.clone();

    let s_tag = if let Some(entity) = context.source().world().get_block_entity(block_pos) {
        let tag = entity.save_with_full_metadata().to_nbt_tag();

        if let Some(t) = get_single_tag(&tag, &path)? {
            t
        } else {
            return Err(CommandSyntaxError::dynamic(
                translations::COMMANDS_DATA_GET_UNKNOWN
                    .message([TextComponent::plain(path.as_str().to_string())])
                    .component(),
            ));
        }
    } else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_DATA_BLOCK_INVALID,
        )));
    };

    Ok((s_tag, block_pos, path))
}

fn get_numeric_value(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<i32, CommandSyntaxError> {
    let (tag, block_pos, path) = get_single(context, arg)?;
    let scale = context.double(SCALE_ARG)?;

    if let Some(result) = process_numeric_arg(tag, scale) {
        context.source().send_success(
            &print_success_scaled(&path, &block_pos, scale, result),
            false,
        );

        Ok(result)
    } else {
        Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_DATA_GET_INVALID
                .message([TextComponent::plain(path.as_str().to_string())]),
        ))
    }
}

fn print_success(data: &NbtTag, pos: &BlockPos) -> TextComponent {
    translations::COMMANDS_DATA_BLOCK_QUERY
        .message([
            TextComponent::plain(pos.x().to_string()),
            TextComponent::plain(pos.y().to_string()),
            TextComponent::plain(pos.z().to_string()),
            command_nbt_component(data, false),
        ])
        .component()
}

fn print_success_scaled(path: &NbtPath, pos: &BlockPos, scale: f64, val: i32) -> TextComponent {
    translations::COMMANDS_DATA_BLOCK_GET
        .message([
            TextComponent::plain(path.as_str().to_string()),
            TextComponent::plain(pos.x().to_string()),
            TextComponent::plain(pos.y().to_string()),
            TextComponent::plain(pos.z().to_string()),
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
fn modify_source() -> Builder {
    todo!()
}

pub(super) fn remove_target() -> Builder {
    todo!()
}
pub(super) fn remove_source() -> Builder {
    todo!()
}
