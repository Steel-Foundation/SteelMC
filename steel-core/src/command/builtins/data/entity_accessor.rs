use super::super::super::execution::{
    CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument, literal,
};
use crate::command::brigadier::{CommandNodeBuilder, CommandSyntaxError};
use crate::command::builtins::data::{
    NBT_ARG, PATH_ARG, SCALE_ARG, get_single_tag, merge_compounds, path_scale_args,
    process_numeric_arg, process_path_arg,
};
use crate::entity::SharedEntity;
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::nbt::NbtPath;
use steel_utils::text::command_nbt_component;
use steel_utils::translations;
use text_components::TextComponent;

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const ACCESSOR_KEYWORD: &str = "entity";
const TARGET_ARG: &str = "target";
const SOURCE_ARG: &str = "source";

pub(super) fn get_target() -> Builder {
    literal(ACCESSOR_KEYWORD).then(
        argument(TARGET_ARG, SteelArgumentType::entity())
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
        argument(SOURCE_ARG, SteelArgumentType::entity())
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
    let entity = context.entity(&arg)?;

    let tag = entity.nbt_for_data_compare().to_nbt_tag();

    context
        .source()
        .send_success(&print_success(&tag, &entity), false);

    Ok(1)
}

fn get_tag_from_path(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<i32, CommandSyntaxError> {
    let (tag, entity, ..) = get_single(context, arg)?;

    context
        .source()
        .send_success(&print_success(&tag, &entity), false);

    Ok(process_path_arg(tag))
}

fn get_single(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<(NbtTag, SharedEntity, NbtPath), CommandSyntaxError> {
    let entity = context.entity(arg)?;
    let path = context.nbt_path(PATH_ARG)?.clone();

    let tag = entity.nbt_for_data_compare().to_nbt_tag();

    let Some(s_tag) = get_single_tag(&tag, &path)? else {
        return Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_DATA_GET_UNKNOWN
                .message([TextComponent::plain(path.as_str().to_string())])
                .component(),
        ));
    };

    Ok((s_tag, entity, path))
}

fn get_numeric_value(
    context: &SteelCommandContext<CommandSource>,
    arg: &str,
) -> Result<i32, CommandSyntaxError> {
    let (tag, entity, path) = get_single(context, arg)?;
    let scale = context.double(SCALE_ARG)?;

    if let Some(result) = process_numeric_arg(tag, scale) {
        context
            .source()
            .send_success(&print_success_scaled(&path, &entity, scale, result), false);

        Ok(result)
    } else {
        Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_DATA_GET_INVALID
                .message([TextComponent::plain(path.as_str().to_string())]),
        ))
    }
}

pub(super) fn merge_target() -> Builder {
    literal(ACCESSOR_KEYWORD)
        //     TODO Implement entity merging (then uncomment code below)
        .then(
            argument(TARGET_ARG, SteelArgumentType::entity()).then(
                argument(NBT_ARG, SteelArgumentType::nbt_compound())
                    .executes(move |ctx| merge_data(ctx, TARGET_ARG.to_string())),
            ),
        )
}
pub(super) fn merge_source() -> Builder {
    literal(ACCESSOR_KEYWORD)
        //     TODO Implement entity merging (then uncomment code below)
        .then(
            argument(SOURCE_ARG, SteelArgumentType::entity()).then(
                argument(NBT_ARG, SteelArgumentType::nbt_compound())
                    .executes(move |ctx| merge_data(ctx, TARGET_ARG.to_string())),
            ),
        )
}

fn merge_data(
    context: &SteelCommandContext<CommandSource>,
    arg: String,
) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let entity: SharedEntity = context.entity(&arg)?;
    let nbt_compound = context.nbt_compound(NBT_ARG)?.clone();

    let old_data = entity.nbt_for_data_compare();

    let merged = merge_compounds(&old_data, &nbt_compound);

    if old_data == merged {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_DATA_MERGE_FAILED,
        )));
    }

    set_data(merged, &entity)?;

    source.send_success(&modified_success(&entity), true);
    Ok(1)
}

pub(super) fn set_data(
    compound: NbtCompound,
    entity: &SharedEntity,
) -> Result<(), CommandSyntaxError> {
    if entity.as_player().is_none() {
        entity.load(compound);
        return Ok(());
    }

    Err(CommandSyntaxError::dynamic(TextComponent::from(
        &translations::COMMANDS_DATA_ENTITY_INVALID,
    )))
}

fn print_success(data: &NbtTag, entity: &SharedEntity) -> TextComponent {
    translations::COMMANDS_DATA_ENTITY_QUERY
        .message([
            TextComponent::plain(entity.display_name().to_string()),
            command_nbt_component(data, false),
        ])
        .component()
}

fn print_success_scaled(
    path: &NbtPath,
    entity: &SharedEntity,
    scale: f64,
    val: i32,
) -> TextComponent {
    translations::COMMANDS_DATA_ENTITY_GET
        .message([
            TextComponent::plain(path.as_str().to_string()),
            TextComponent::plain(entity.display_name().to_string()),
            TextComponent::plain(format!("{scale:.2}")),
            TextComponent::plain(val.to_string()),
        ])
        .component()
}

fn modified_success(entity: &SharedEntity) -> TextComponent {
    translations::COMMANDS_DATA_ENTITY_MODIFIED
        .message([entity.display_name()])
        .component()
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
