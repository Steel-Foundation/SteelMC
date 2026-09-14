//! Vanilla server-version reporting command.

use steel_registry::packets::CURRENT_MC_PROTOCOL;
use steel_utils::{Identifier, translations, version};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("version"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("version").executes(send_version)
}

/// Reports the targeted vanilla version the way `VersionCommand#dumpVersion`
/// does, using the `version.json` the build script extracts from the target
/// server jar for everything but the protocol number (already tracked by
/// `steel_registry::packets::CURRENT_MC_PROTOCOL`).
#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn send_version(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    source.send_system_message(&TextComponent::from(&translations::COMMANDS_VERSION_HEADER));
    source.send_system_message(
        &translations::COMMANDS_VERSION_ID
            .message([version::VERSION_ID])
            .component(),
    );
    source.send_system_message(
        &translations::COMMANDS_VERSION_NAME
            .message([version::VERSION_NAME])
            .component(),
    );
    source.send_system_message(
        &translations::COMMANDS_VERSION_DATA
            .message([version::DATA_VERSION.to_string()])
            .component(),
    );
    source.send_system_message(
        &translations::COMMANDS_VERSION_SERIES
            .message([version::DATA_VERSION_SERIES])
            .component(),
    );
    source.send_system_message(
        &translations::COMMANDS_VERSION_PROTOCOL
            .message([
                CURRENT_MC_PROTOCOL.to_string(),
                format!("0x{CURRENT_MC_PROTOCOL:x}"),
            ])
            .component(),
    );
    source.send_system_message(
        &translations::COMMANDS_VERSION_BUILD_TIME
            .message([version::BUILD_TIME])
            .component(),
    );
    source.send_system_message(
        &translations::COMMANDS_VERSION_PACK_RESOURCE
            .message([version::RESOURCE_PACK_VERSION])
            .component(),
    );
    source.send_system_message(
        &translations::COMMANDS_VERSION_PACK_DATA
            .message([version::DATA_PACK_VERSION])
            .component(),
    );
    let stable = if version::STABLE {
        &translations::COMMANDS_VERSION_STABLE_YES
    } else {
        &translations::COMMANDS_VERSION_STABLE_NO
    };
    source.send_system_message(&TextComponent::from(stable));
    Ok(1)
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;

    #[test]
    fn version_is_a_bare_executable_literal() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(version) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "version")
            })
        }) else {
            panic!("version root should exist");
        };
        let Some(node) = dispatcher.node(version) else {
            panic!("version root node should exist");
        };
        assert!(node.is_executable());
        assert!(node.argument_type().is_none());
    }
}
