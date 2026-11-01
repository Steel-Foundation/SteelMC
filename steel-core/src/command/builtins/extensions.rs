//! Steel's `/extensions [name]`: installed native extensions and embedded extensions.
//!
//! Not a vanilla command. Bukkit splits this into `/plugins` and `/version <plugin>`; Steel keeps
//! vanilla's `/version` unchanged and shows details through `/extensions <name>` instead.

use steel_utils::Identifier;
use text_components::{Modifier, TextComponent, format::Color};

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, argument, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    // `plugins` keeps the Bukkit name working for administrators used to it.
    CommandRegistration::new(Identifier::from_steel("extensions"), |_| command())
        .alias("plugins")
        .default_access()
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("extensions")
        .executes(list)
        .then(argument("name", ArgumentType::word()).executes(details))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn list(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let infos = source.server().extension_infos();
    let names: Vec<String> = infos
        .iter()
        .map(|info| format!("{} v{}", info.name, info.version))
        .collect();
    source.send_system_message(
        &TextComponent::plain(format!("Extensions ({}): ", infos.len()))
            .color(Color::Gold)
            .add_child(TextComponent::plain(names.join(", ")).color(Color::Green)),
    );
    Ok(i32::try_from(infos.len()).unwrap_or(i32::MAX))
}

fn details(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let name = context.string("name")?;
    let Some(info) = source
        .server()
        .extension_infos()
        .iter()
        .find(|info| info.name.eq_ignore_ascii_case(name))
    else {
        return Err(CommandSyntaxError::dynamic(format!(
            "No extension named {name}"
        )));
    };
    let join = |list: &[String]| {
        if list.is_empty() {
            "(none)".to_owned()
        } else {
            list.join(", ")
        }
    };
    for line in [
        format!("{} version {}", info.name, info.version),
        format!("Built for extension API {}", info.api_version),
        format!("Depends: {}", join(&info.depends)),
        format!("Soft depends: {}", join(&info.soft_depends)),
        format!("Loaded from: {}", info.source),
    ] {
        source.send_system_message(&TextComponent::plain(line).color(Color::Gold));
    }
    Ok(1)
}
