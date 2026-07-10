use rustc_hash::FxHashMap;
use steel_protocol::packets::game::{
    ArgumentStringTypeBehavior, ArgumentType as ProtocolArgumentType, CCommandSuggestions,
    CCommands, CommandNode as ProtocolCommandNode, CommandNodeInfo, SuggestionEntry,
    SuggestionType,
};
use thiserror::Error;

use super::brigadier::{
    ArgumentType, CommandDispatcher, CommandRuntime, NodeId, NodeKind, StringType, Suggestions,
};
use super::execution::SteelArgumentType;

const MAX_COMMAND_SUGGESTIONS: usize = 1000;

/// Converts Brigadier suggestions to the vanilla command-suggestion packet.
pub(crate) fn command_suggestions_packet(
    transaction_id: i32,
    suggestions: &Suggestions,
) -> CCommandSuggestions {
    let range = suggestions.range();
    // Serverbound command suggestions are bounded to 32,500 bytes, so their
    // UTF-16 indices always fit the packet's signed VarInts.
    let start = range.start() as i32;
    let length = range.len() as i32;
    let entries = suggestions
        .list()
        .iter()
        .take(MAX_COMMAND_SUGGESTIONS)
        .map(|suggestion| match suggestion.tooltip() {
            Some(tooltip) => SuggestionEntry::with_tooltip(suggestion.text(), tooltip.clone()),
            None => SuggestionEntry::new(suggestion.text()),
        })
        .collect();
    CCommandSuggestions::new(transaction_id, start, length, entries)
}

/// A filtered Brigadier graph could not be represented by the vanilla packet.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum CommandTreeProjectionError {
    #[error("command graph contains an unknown node {0:?}")]
    UnknownNode(NodeId),
    #[error("command graph contains more nodes than the protocol can index")]
    TooManyNodes,
    #[error("argument node {0:?} has no argument parser")]
    MissingArgumentType(NodeId),
    #[error("visible command node {node:?} redirects to filtered node {target:?}")]
    HiddenRedirectTarget { node: NodeId, target: NodeId },
}

/// Builds the command graph visible to one source using vanilla packet indices.
pub(crate) fn command_tree_packet<S, R>(
    dispatcher: &CommandDispatcher<S, R>,
    source: &S,
) -> Result<CCommands, CommandTreeProjectionError>
where
    R: CommandRuntime<S>,
    R::Argument: CommandArgumentProtocol,
{
    let visible = visible_nodes(dispatcher, source)?;
    let mut indices = FxHashMap::default();
    for (index, node) in visible.iter().copied().enumerate() {
        let Ok(index) = i32::try_from(index) else {
            return Err(CommandTreeProjectionError::TooManyNodes);
        };
        indices.insert(node, index);
    }

    let mut nodes = Vec::with_capacity(visible.len());
    for node_id in visible {
        let node = dispatcher
            .node(node_id)
            .ok_or(CommandTreeProjectionError::UnknownNode(node_id))?;
        let children = dispatcher
            .children(node_id)
            .ok_or(CommandTreeProjectionError::UnknownNode(node_id))?
            .iter()
            .filter_map(|child| indices.get(child).copied())
            .collect();
        let redirect = match node.redirect() {
            Some(target) => Some(indices.get(&target).copied().ok_or(
                CommandTreeProjectionError::HiddenRedirectTarget {
                    node: node_id,
                    target,
                },
            )?),
            None => None,
        };

        let mut info = CommandNodeInfo::new(children);
        if node.is_executable() {
            info = info.executable();
        }
        if node.is_restricted() {
            info = info.restricted();
        }
        if let Some(target) = redirect {
            info = info.redirect(target);
        }

        let projected = match node.kind() {
            NodeKind::Root => {
                let mut root = ProtocolCommandNode::new_root();
                root.set_children(
                    dispatcher
                        .children(node_id)
                        .ok_or(CommandTreeProjectionError::UnknownNode(node_id))?
                        .iter()
                        .filter_map(|child| indices.get(child).copied())
                        .collect(),
                );
                root
            }
            NodeKind::Literal => ProtocolCommandNode::new_literal(info, node.name().to_owned()),
            NodeKind::Argument => {
                let argument = node
                    .argument_type()
                    .ok_or(CommandTreeProjectionError::MissingArgumentType(node_id))?;
                ProtocolCommandNode::new_argument(
                    info,
                    node.name().to_owned(),
                    argument.protocol_argument(),
                )
            }
        };
        nodes.push(projected);
    }

    Ok(CCommands {
        nodes,
        root_index: 0,
    })
}

fn visible_nodes<S, R>(
    dispatcher: &CommandDispatcher<S, R>,
    source: &S,
) -> Result<Vec<NodeId>, CommandTreeProjectionError>
where
    R: CommandRuntime<S>,
{
    let mut visible = Vec::new();
    let mut pending = vec![dispatcher.root()];
    while let Some(node_id) = pending.pop() {
        visible.push(node_id);
        let children = dispatcher
            .children(node_id)
            .ok_or(CommandTreeProjectionError::UnknownNode(node_id))?;
        for child in children.iter().rev() {
            let node = dispatcher
                .node(*child)
                .ok_or(CommandTreeProjectionError::UnknownNode(*child))?;
            if node.allows(source) {
                pending.push(*child);
            }
        }
    }
    Ok(visible)
}

pub(crate) trait CommandArgumentProtocol {
    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<SuggestionType>);
}

impl CommandArgumentProtocol for ArgumentType {
    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<SuggestionType>) {
        (protocol_argument_type(self), None)
    }
}

impl CommandArgumentProtocol for SteelArgumentType {
    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<SuggestionType>) {
        match self {
            SteelArgumentType::Primitive(argument) => argument.protocol_argument(),
            SteelArgumentType::Time { minimum } => {
                (ProtocolArgumentType::Time { min: *minimum }, None)
            }
            SteelArgumentType::BlockPos => (ProtocolArgumentType::BlockPos, None),
            SteelArgumentType::Vec3 { .. } => (ProtocolArgumentType::Vec3, None),
            SteelArgumentType::Rotation => (ProtocolArgumentType::Rotation, None),
            SteelArgumentType::Entity {
                single,
                players_only,
            } => (
                ProtocolArgumentType::Entity {
                    flags: u8::from(*single) | (u8::from(*players_only) << 1),
                },
                Some(SuggestionType::AskServer),
            ),
            SteelArgumentType::GameMode => (ProtocolArgumentType::Gamemode, None),
            SteelArgumentType::SummonableEntity => (
                ProtocolArgumentType::Resource {
                    identifier: "minecraft:entity_type",
                },
                Some(SuggestionType::SummonableEntities),
            ),
            SteelArgumentType::WorldClock => (
                ProtocolArgumentType::Resource {
                    identifier: "minecraft:world_clock",
                },
                None,
            ),
            SteelArgumentType::Timeline { .. } => (
                ProtocolArgumentType::Resource {
                    identifier: "minecraft:timeline",
                },
                Some(SuggestionType::AskServer),
            ),
            SteelArgumentType::Domain | SteelArgumentType::TimeMarker { .. } => (
                ProtocolArgumentType::ResourceLocation,
                Some(SuggestionType::AskServer),
            ),
        }
    }
}

fn protocol_argument_type(argument: &ArgumentType) -> ProtocolArgumentType {
    match *argument {
        ArgumentType::Bool => ProtocolArgumentType::Bool,
        ArgumentType::Integer { minimum, maximum } => ProtocolArgumentType::Integer {
            min: (minimum != i32::MIN).then_some(minimum),
            max: (maximum != i32::MAX).then_some(maximum),
        },
        ArgumentType::Long { minimum, maximum } => ProtocolArgumentType::Long {
            min: (minimum != i64::MIN).then_some(minimum),
            max: (maximum != i64::MAX).then_some(maximum),
        },
        ArgumentType::Float { minimum, maximum } => ProtocolArgumentType::Float {
            min: (minimum.to_bits() != (-f32::MAX).to_bits()).then_some(minimum),
            max: (maximum.to_bits() != f32::MAX.to_bits()).then_some(maximum),
        },
        ArgumentType::Double { minimum, maximum } => ProtocolArgumentType::Double {
            min: (minimum.to_bits() != (-f64::MAX).to_bits()).then_some(minimum),
            max: (maximum.to_bits() != f64::MAX.to_bits()).then_some(maximum),
        },
        ArgumentType::String(string_type) => ProtocolArgumentType::String {
            behavior: match string_type {
                StringType::Word => ArgumentStringTypeBehavior::SingleWord,
                StringType::QuotablePhrase => ArgumentStringTypeBehavior::QuotablePhrase,
                StringType::GreedyPhrase => ArgumentStringTypeBehavior::GreedyPhrase,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandArgumentProtocol, CommandTreeProjectionError, MAX_COMMAND_SUGGESTIONS,
        command_suggestions_packet, command_tree_packet, protocol_argument_type,
    };
    use crate::command::brigadier::{
        ArgumentType, CommandDispatcher, CommandNodeBuilder, CommandRequirement, NodeId,
        StringRange, StringReader, StringType, Suggestion, Suggestions, argument, literal,
    };
    use crate::command::execution::SteelArgumentType;
    use steel_protocol::packets::game::{
        ArgumentStringTypeBehavior, ArgumentType as ProtocolArgumentType,
        CommandNode as ProtocolCommandNode, SuggestionType,
    };
    use text_components::TextComponent;

    #[derive(Clone, Copy)]
    struct TestSource {
        authorized: bool,
        in_context: bool,
    }

    fn register(
        dispatcher: &mut CommandDispatcher<TestSource>,
        builder: CommandNodeBuilder<TestSource>,
    ) -> NodeId {
        let Ok(node) = dispatcher.register(builder) else {
            panic!("test command should register");
        };
        node
    }

    #[test]
    fn leading_slash_remains_in_the_packet_replacement_range() {
        let mut dispatcher = CommandDispatcher::<()>::new();
        assert!(dispatcher.register(literal("help")).is_ok());
        let mut reader = StringReader::new("/he");
        assert!(reader.skip());

        let parse = dispatcher.parse_reader(reader, ());
        let suggestions = dispatcher.completion_suggestions(&parse);
        let Ok(suggestions) = suggestions else {
            panic!("slash-prefixed suggestions should build");
        };
        let packet = command_suggestions_packet(7, &suggestions);

        assert_eq!(packet.id, 7);
        assert_eq!(packet.start, 1);
        assert_eq!(packet.length, 2);
        assert_eq!(packet.suggestions.len(), 1);
        assert_eq!(packet.suggestions[0].text, "help");
    }

    #[test]
    fn packet_projection_preserves_utf16_range_and_tooltip() {
        let tooltip = TextComponent::plain("details");
        let suggestions = Suggestions::new(
            StringRange::between(2, 4),
            vec![Suggestion::with_tooltip(
                StringRange::between(2, 4),
                "value",
                tooltip.clone(),
            )],
        );

        let packet = command_suggestions_packet(11, &suggestions);

        assert_eq!(packet.start, 2);
        assert_eq!(packet.length, 2);
        assert_eq!(packet.suggestions[0].text, "value");
        assert_eq!(packet.suggestions[0].tooltip.as_ref(), Some(&tooltip));
    }

    #[test]
    fn packet_projection_applies_vanillas_suggestion_limit() {
        let range = StringRange::at(0);
        let suggestions = Suggestions::new(
            range,
            (0..=MAX_COMMAND_SUGGESTIONS)
                .map(|index| Suggestion::new(range, index.to_string()))
                .collect(),
        );

        let packet = command_suggestions_packet(1, &suggestions);

        assert_eq!(packet.suggestions.len(), MAX_COMMAND_SUGGESTIONS);
        assert_eq!(packet.suggestions[0].text, "0");
        assert_eq!(packet.suggestions[MAX_COMMAND_SUGGESTIONS - 1].text, "999");
    }

    #[test]
    fn command_tree_filters_both_requirement_kinds_but_marks_only_authorization() {
        let mut dispatcher = CommandDispatcher::new();
        register(&mut dispatcher, literal("public").executes(|_| Ok(1)));
        register(
            &mut dispatcher,
            literal("admin")
                .requires(CommandRequirement::authorization(|source: &TestSource| {
                    source.authorized
                }))
                .executes(|_| Ok(1)),
        );
        register(
            &mut dispatcher,
            literal("nearby")
                .requires(CommandRequirement::contextual(|source: &TestSource| {
                    source.in_context
                }))
                .executes(|_| Ok(1)),
        );

        let denied = command_tree_packet(
            &dispatcher,
            &TestSource {
                authorized: false,
                in_context: false,
            },
        );
        let Ok(denied) = denied else {
            panic!("denied command tree should project");
        };
        assert_eq!(denied.nodes.len(), 2);
        let ProtocolCommandNode::Root { children } = &denied.nodes[0] else {
            panic!("first projected node should be the root");
        };
        assert_eq!(children, &[1]);

        let allowed = command_tree_packet(
            &dispatcher,
            &TestSource {
                authorized: true,
                in_context: true,
            },
        );
        let Ok(allowed) = allowed else {
            panic!("allowed command tree should project");
        };
        assert_eq!(allowed.nodes.len(), 4);
        let ProtocolCommandNode::Literal {
            name,
            is_restricted,
            ..
        } = &allowed.nodes[2]
        else {
            panic!("authorization command should be a literal");
        };
        assert_eq!(name, "admin");
        assert!(*is_restricted);
        let ProtocolCommandNode::Literal {
            name,
            is_restricted,
            ..
        } = &allowed.nodes[3]
        else {
            panic!("context command should be a literal");
        };
        assert_eq!(name, "nearby");
        assert!(!is_restricted);
    }

    #[test]
    fn command_tree_remaps_redirects_after_filtering() {
        let mut dispatcher = CommandDispatcher::new();
        let target = register(&mut dispatcher, literal("target").executes(|_| Ok(1)));
        register(
            &mut dispatcher,
            literal("hidden").requires(CommandRequirement::contextual(|source: &TestSource| {
                source.in_context
            })),
        );
        register(&mut dispatcher, literal("alias").redirects(target));

        let packet = command_tree_packet(
            &dispatcher,
            &TestSource {
                authorized: false,
                in_context: false,
            },
        );
        let Ok(packet) = packet else {
            panic!("visible redirect target should project");
        };
        assert_eq!(packet.nodes.len(), 3);
        let ProtocolCommandNode::Literal {
            name, redirects_to, ..
        } = &packet.nodes[2]
        else {
            panic!("alias should be a literal");
        };
        assert_eq!(name, "alias");
        assert_eq!(*redirects_to, Some(1));
    }

    #[test]
    fn command_tree_rejects_visible_redirects_to_filtered_targets() {
        let mut dispatcher = CommandDispatcher::new();
        let target = register(
            &mut dispatcher,
            literal("target").requires(CommandRequirement::contextual(|source: &TestSource| {
                source.in_context
            })),
        );
        let alias = register(&mut dispatcher, literal("alias").redirects(target));

        let result = command_tree_packet(
            &dispatcher,
            &TestSource {
                authorized: false,
                in_context: false,
            },
        );

        assert!(matches!(
            result,
            Err(CommandTreeProjectionError::HiddenRedirectTarget {
                node,
                target: hidden_target,
            }) if node == alias && hidden_target == target
        ));
    }

    #[test]
    fn command_tree_projects_executable_primitive_arguments() {
        let mut dispatcher = CommandDispatcher::new();
        register(
            &mut dispatcher,
            literal("number")
                .then(argument("value", ArgumentType::integer(1, i32::MAX)).executes(|_| Ok(1))),
        );

        let packet = command_tree_packet(
            &dispatcher,
            &TestSource {
                authorized: false,
                in_context: false,
            },
        );
        let Ok(packet) = packet else {
            panic!("primitive argument tree should project");
        };
        let ProtocolCommandNode::Argument {
            name,
            is_executable,
            parser: ProtocolArgumentType::Integer { min, max },
            ..
        } = &packet.nodes[2]
        else {
            panic!("third node should be an integer argument");
        };
        assert_eq!(name, "value");
        assert!(*is_executable);
        assert_eq!(*min, Some(1));
        assert_eq!(*max, None);
    }

    #[test]
    fn primitive_argument_projection_omits_default_bounds_and_maps_strings() {
        assert!(matches!(
            protocol_argument_type(&ArgumentType::float(-f32::MAX, f32::MAX)),
            ProtocolArgumentType::Float {
                min: None,
                max: None
            }
        ));
        assert!(matches!(
            protocol_argument_type(&ArgumentType::String(StringType::GreedyPhrase)),
            ProtocolArgumentType::String {
                behavior: ArgumentStringTypeBehavior::GreedyPhrase
            }
        ));
    }

    #[test]
    fn steel_time_argument_projects_its_minimum() {
        let (argument, suggestions) = SteelArgumentType::time(1).protocol_argument();
        let ProtocolArgumentType::Time { min } = argument else {
            panic!("Steel time parser should project as the protocol time argument");
        };

        assert_eq!(min, 1);
        assert!(suggestions.is_none());
    }

    #[test]
    fn steel_coordinate_arguments_project_vanilla_parsers() {
        let (block_pos, block_pos_suggestions) = SteelArgumentType::block_pos().protocol_argument();
        assert!(matches!(block_pos, ProtocolArgumentType::BlockPos));
        assert!(block_pos_suggestions.is_none());

        let (vec3, vec3_suggestions) = SteelArgumentType::vec3(true).protocol_argument();
        assert!(matches!(vec3, ProtocolArgumentType::Vec3));
        assert!(vec3_suggestions.is_none());

        let (rotation, rotation_suggestions) = SteelArgumentType::rotation().protocol_argument();
        assert!(matches!(rotation, ProtocolArgumentType::Rotation));
        assert!(rotation_suggestions.is_none());
    }

    #[test]
    fn steel_entity_arguments_project_vanilla_flags() {
        for (argument, expected_flags) in [
            (SteelArgumentType::entities(), 0),
            (SteelArgumentType::entity(), 1),
            (SteelArgumentType::players(), 2),
            (SteelArgumentType::player(), 3),
        ] {
            let (argument, suggestions) = argument.protocol_argument();
            assert!(matches!(
                argument,
                ProtocolArgumentType::Entity { flags } if flags == expected_flags
            ));
            assert!(matches!(suggestions, Some(SuggestionType::AskServer)));
        }
    }

    #[test]
    fn steel_game_mode_argument_projects_vanillas_parser() {
        let (argument, suggestions) = SteelArgumentType::game_mode().protocol_argument();

        assert!(matches!(argument, ProtocolArgumentType::Gamemode));
        assert!(suggestions.is_none());
    }

    #[test]
    fn steel_domain_argument_uses_server_resource_suggestions() {
        let (domain, suggestions) = SteelArgumentType::domain().protocol_argument();
        assert!(matches!(domain, ProtocolArgumentType::ResourceLocation));
        assert!(matches!(suggestions, Some(SuggestionType::AskServer)));
    }

    #[test]
    fn summonable_entity_argument_projects_vanillas_resource_and_suggestions() {
        let (entity, suggestions) = SteelArgumentType::summonable_entity().protocol_argument();
        assert!(matches!(
            entity,
            ProtocolArgumentType::Resource {
                identifier: "minecraft:entity_type"
            }
        ));
        assert!(matches!(
            suggestions,
            Some(SuggestionType::SummonableEntities)
        ));
    }

    #[test]
    fn steel_clock_arguments_project_vanilla_resource_parsers() {
        let (clock, clock_suggestions) = SteelArgumentType::world_clock().protocol_argument();
        assert!(matches!(
            clock,
            ProtocolArgumentType::Resource {
                identifier: "minecraft:world_clock"
            }
        ));
        assert!(clock_suggestions.is_none());

        let (timeline, timeline_suggestions) =
            SteelArgumentType::timeline(Some("clock")).protocol_argument();
        assert!(matches!(
            timeline,
            ProtocolArgumentType::Resource {
                identifier: "minecraft:timeline"
            }
        ));
        assert!(matches!(
            timeline_suggestions,
            Some(SuggestionType::AskServer)
        ));

        let (marker, marker_suggestions) = SteelArgumentType::time_marker(None).protocol_argument();
        assert!(matches!(marker, ProtocolArgumentType::ResourceLocation));
        assert!(matches!(
            marker_suggestions,
            Some(SuggestionType::AskServer)
        ));
    }
}
