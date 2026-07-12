//! Steel-owned built-in command declarations.

mod clear;
mod difficulty;
mod domain;
mod enchant;
mod execute;
mod experience;
mod fly;
pub(crate) mod gamemode;
mod gamerule;
mod give;
mod kill;
mod list;
mod locate;
mod operator;
mod perms;
mod return_command;
mod seed;
mod setworldspawn;
mod stop;
mod summon;
mod teleport;
mod tellraw;
mod tick;
mod time;
mod weather;

pub(crate) use difficulty::player_can_change_difficulty;

use super::{
    CommandRegistry,
    execution::CommandSource,
    registration::{
        CommandDispatcherBuilder, CommandRegistrationError,
        ENTITY_SELECTOR_ADVANCED_PERMISSION_KEY, ENTITY_SELECTOR_PERMISSION_KEY,
        RegisteredCommandDispatcher,
    },
};
#[cfg(test)]
use super::{brigadier::CommandDispatcher, execution::SteelCommandRuntime};

#[cfg(test)]
pub(crate) fn create_dispatcher()
-> Result<CommandDispatcher<CommandSource, SteelCommandRuntime>, CommandRegistrationError> {
    create_registered_dispatcher(CommandRegistry::new()).map(|registered| registered.dispatcher)
}

pub(crate) fn create_registered_dispatcher(
    extension_commands: CommandRegistry,
) -> Result<RegisteredCommandDispatcher<CommandSource>, CommandRegistrationError> {
    let mut builder = CommandDispatcherBuilder::new();
    builder.declare_permission(ENTITY_SELECTOR_PERMISSION_KEY)?;
    builder.declare_permission(ENTITY_SELECTOR_ADVANCED_PERMISSION_KEY)?;
    builder.declare_permission(perms::MANAGE_ALL_PERMISSION)?;
    builder.declare_permission(perms::GROUP_ALL_PERMISSION)?;
    builder.declare_permission(perms::METADATA_PERMISSION)?;
    builder.register(clear::registration())?;
    builder.register(operator::deop_registration())?;
    builder.register(difficulty::registration())?;
    builder.register(domain::registration())?;
    builder.register(enchant::registration())?;
    builder.register(execute::registration())?;
    builder.register(experience::registration())?;
    builder.register(fly::registration())?;
    builder.register(gamemode::registration()?)?;
    builder.register(gamerule::registration())?;
    builder.register(give::registration())?;
    builder.register(kill::registration())?;
    builder.register(list::registration())?;
    builder.register(locate::registration())?;
    builder.register(operator::op_registration())?;
    builder.register(perms::registration())?;
    builder.register(return_command::registration())?;
    builder.register(seed::registration())?;
    builder.register(setworldspawn::registration())?;
    builder.register(stop::registration())?;
    builder.register(summon::registration())?;
    builder.register(teleport::registration())?;
    builder.register(tellraw::registration())?;
    builder.register(tick::registration())?;
    builder.register(time::registration())?;
    builder.register(weather::registration())?;
    builder.extend(extension_commands.into_inner())?;
    builder.build_with_permissions()
}

#[cfg(test)]
mod tests {
    use super::{create_dispatcher, create_registered_dispatcher};
    use crate::command::brigadier::ArgumentType;
    use crate::command::execution::SteelArgumentType;
    use crate::command::{
        CommandRegistration as ExtensionCommandRegistration, CommandRegistry,
        literal as extension_literal,
    };
    use steel_registry::test_support::init_test_registry;
    use steel_utils::Identifier;

    #[test]
    fn first_builtin_slice_has_the_expected_graph_shape() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(roots) = dispatcher.children(dispatcher.root()) else {
            panic!("dispatcher root should exist");
        };
        let names = roots
            .iter()
            .map(|root| {
                let Some(root) = dispatcher.node(*root) else {
                    panic!("registered root should exist");
                };
                root.name()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            [
                "clear",
                "deop",
                "difficulty",
                "domain",
                "enchant",
                "execute",
                "experience",
                "xp",
                "fly",
                "gamemode",
                "gamerule",
                "give",
                "kill",
                "list",
                "locate",
                "op",
                "perms",
                "return",
                "seed",
                "setworldspawn",
                "stop",
                "summon",
                "teleport",
                "tp",
                "tellraw",
                "tick",
                "time",
                "weather"
            ]
        );

        let Some(list) = roots.iter().copied().find(|root| {
            dispatcher
                .node(*root)
                .is_some_and(|node| node.name() == "list")
        }) else {
            panic!("list root should exist");
        };
        let Some(list_node) = dispatcher.node(list) else {
            panic!("list root should exist");
        };
        assert!(list_node.is_executable());
        let Some(list_children) = dispatcher.children(list) else {
            panic!("list children should exist");
        };
        assert_eq!(list_children.len(), 1);
        assert!(
            dispatcher
                .node(list_children[0])
                .is_some_and(|node| { node.name() == "uuids" && node.is_executable() })
        );

        let Some(weather) = roots.iter().copied().find(|root| {
            dispatcher
                .node(*root)
                .is_some_and(|node| node.name() == "weather")
        }) else {
            panic!("weather root should exist");
        };
        let Some(weather_children) = dispatcher.children(weather) else {
            panic!("weather children should exist");
        };
        let weather_names = weather_children
            .iter()
            .map(|child| {
                let Some(node) = dispatcher.node(*child) else {
                    panic!("weather literal should exist");
                };
                assert!(node.is_executable());
                let Some(duration_children) = dispatcher.children(*child) else {
                    panic!("weather duration child should exist");
                };
                assert_eq!(duration_children.len(), 1);
                let Some(duration) = dispatcher.node(duration_children[0]) else {
                    panic!("weather duration node should exist");
                };
                assert_eq!(duration.name(), "duration");
                assert!(duration.is_executable());
                assert_eq!(duration.argument_type(), Some(&SteelArgumentType::time(1)));
                node.name()
            })
            .collect::<Vec<_>>();
        assert_eq!(weather_names, ["clear", "rain", "thunder"]);
    }

    #[test]
    fn startup_extensions_merge_after_builtins_with_namespaced_collision_fallbacks() {
        init_test_registry();
        let mut extensions = CommandRegistry::new();
        let registration =
            ExtensionCommandRegistration::new(Identifier::new("steel_test", "stop"), || {
                extension_literal("stop").executes(|_| Ok(1))
            });
        assert!(extensions.register(registration).is_ok());

        let Ok(registered) = create_registered_dispatcher(extensions) else {
            panic!("built-ins and extension should register atomically");
        };
        let dispatcher = registered.dispatcher;
        let Some(roots) = dispatcher.children(dispatcher.root()) else {
            panic!("dispatcher root should exist");
        };
        let names = roots
            .iter()
            .filter_map(|root| {
                let node = dispatcher.node(*root)?;
                Some(node.name())
            })
            .collect::<Vec<_>>();

        assert!(names.contains(&"stop"));
        assert!(names.contains(&"steel_test:stop"));
        assert!(
            registered
                .permissions
                .iter()
                .any(|permission| permission.as_str() == "steel_test.command.stop")
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one graph-shape test keeps execute paths and redirects directly comparable"
    )]
    fn execute_graph_uses_expected_redirects_and_argument_types() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(execute) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "execute")
            })
        }) else {
            panic!("execute root should exist");
        };
        let Some(execute_node) = dispatcher.node(execute) else {
            panic!("execute root should exist");
        };
        assert!(!execute_node.is_executable());

        let child = |parent, name| {
            let Some(node) = dispatcher.children(parent).and_then(|children| {
                children.iter().copied().find(|child| {
                    dispatcher
                        .node(*child)
                        .is_some_and(|node| node.name() == name)
                })
            }) else {
                panic!("{name} should exist below {parent:?}");
            };
            node
        };

        let run = child(execute, "run");
        let Some(run_node) = dispatcher.node(run) else {
            panic!("execute run should exist");
        };
        assert_eq!(run_node.redirect(), Some(dispatcher.root()));

        for condition in ["if", "unless"] {
            for (path, expected_type) in [
                (
                    [condition, "entity", "entities"],
                    SteelArgumentType::entities(),
                ),
                ([condition, "loaded", "pos"], SteelArgumentType::block_pos()),
            ] {
                let terminal = path
                    .iter()
                    .fold(execute, |parent, name| child(parent, name));
                let Some(node) = dispatcher.node(terminal) else {
                    panic!("execute condition terminal should exist");
                };
                assert!(node.is_executable());
                assert_eq!(node.redirect(), Some(execute));
                assert_eq!(node.argument_type(), Some(&expected_type));
            }

            let biome = child(child(child(execute, condition), "biome"), "pos");
            let biome = child(biome, "biome");
            let Some(biome_node) = dispatcher.node(biome) else {
                panic!("execute biome condition terminal should exist");
            };
            assert!(biome_node.is_executable());
            assert_eq!(biome_node.redirect(), Some(execute));
            assert_eq!(
                biome_node.argument_type(),
                Some(&SteelArgumentType::biome_or_tag())
            );

            let block = child(child(child(execute, condition), "block"), "pos");
            let block = child(block, "block");
            let Some(block_node) = dispatcher.node(block) else {
                panic!("execute block condition terminal should exist");
            };
            assert!(block_node.is_executable());
            assert_eq!(block_node.redirect(), Some(execute));
            assert_eq!(
                block_node.argument_type(),
                Some(&SteelArgumentType::block_predicate())
            );

            let blocks = child(child(execute, condition), "blocks");
            let start = child(blocks, "start");
            let end = child(start, "end");
            let destination = child(end, "destination");
            for position in [start, end, destination] {
                assert_eq!(
                    dispatcher
                        .node(position)
                        .and_then(|node| node.argument_type()),
                    Some(&SteelArgumentType::block_pos())
                );
            }
            for mode in ["all", "masked"] {
                let terminal = child(destination, mode);
                let Some(node) = dispatcher.node(terminal) else {
                    panic!("execute blocks mode should exist");
                };
                assert!(node.is_executable());
                assert_eq!(node.redirect(), Some(execute));
            }

            let data = child(child(execute, condition), "data");
            for (provider, source_name, source_type) in [
                ("block", "sourcePos", SteelArgumentType::block_pos()),
                ("entity", "source", SteelArgumentType::entity()),
                ("storage", "source", SteelArgumentType::storage_key()),
            ] {
                let source = child(child(data, provider), source_name);
                assert_eq!(
                    dispatcher
                        .node(source)
                        .and_then(|node| node.argument_type()),
                    Some(&source_type)
                );
                let path = child(source, "path");
                let Some(node) = dispatcher.node(path) else {
                    panic!("execute data path terminal should exist");
                };
                assert!(node.is_executable());
                assert_eq!(node.redirect(), Some(execute));
                assert_eq!(node.argument_type(), Some(&SteelArgumentType::nbt_path()));
            }

            let dimension = child(child(execute, condition), "dimension");
            let dimension = child(dimension, "dimension");
            let Some(node) = dispatcher.node(dimension) else {
                panic!("execute dimension condition terminal should exist");
            };
            assert!(node.is_executable());
            assert_eq!(node.redirect(), Some(execute));
            assert_eq!(node.argument_type(), Some(&SteelArgumentType::world()));

            let score = child(child(execute, condition), "score");
            let target = child(score, "target");
            assert_eq!(
                dispatcher
                    .node(target)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::score_holder())
            );
            let target_objective = child(target, "targetObjective");
            assert_eq!(
                dispatcher
                    .node(target_objective)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::objective())
            );
            for comparison in ["=", "<", "<=", ">", ">="] {
                let source = child(child(target_objective, comparison), "source");
                let source_objective = child(source, "sourceObjective");
                let Some(node) = dispatcher.node(source_objective) else {
                    panic!("score comparison terminal should exist");
                };
                assert!(node.is_executable());
                assert_eq!(node.redirect(), Some(execute));
                assert_eq!(node.argument_type(), Some(&SteelArgumentType::objective()));
            }
            let range = child(child(target_objective, "matches"), "range");
            let Some(range_node) = dispatcher.node(range) else {
                panic!("score range terminal should exist");
            };
            assert!(range_node.is_executable());
            assert_eq!(range_node.redirect(), Some(execute));
            assert_eq!(
                range_node.argument_type(),
                Some(&SteelArgumentType::int_range())
            );
        }

        for store_kind in ["result", "success"] {
            let store = child(child(execute, "store"), store_kind);
            let targets = child(child(store, "score"), "targets");
            assert_eq!(
                dispatcher
                    .node(targets)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::score_holders())
            );
            let objective = child(targets, "objective");
            let Some(objective_node) = dispatcher.node(objective) else {
                panic!("execute store score objective should exist");
            };
            assert_eq!(objective_node.redirect(), Some(execute));
            assert_eq!(
                objective_node.argument_type(),
                Some(&SteelArgumentType::objective())
            );

            for (provider, target_name, target_type) in [
                ("block", "targetPos", SteelArgumentType::block_pos()),
                ("storage", "target", SteelArgumentType::storage_key()),
            ] {
                let target = child(child(store, provider), target_name);
                assert_eq!(
                    dispatcher
                        .node(target)
                        .and_then(|node| node.argument_type()),
                    Some(&target_type)
                );
                let path = child(target, "path");
                assert_eq!(
                    dispatcher.node(path).and_then(|node| node.argument_type()),
                    Some(&SteelArgumentType::nbt_path())
                );
                for data_type in ["int", "float", "short", "long", "double", "byte"] {
                    let scale = child(child(path, data_type), "scale");
                    let Some(scale_node) = dispatcher.node(scale) else {
                        panic!("execute store data scale should exist");
                    };
                    assert_eq!(scale_node.redirect(), Some(execute));
                    assert_eq!(
                        scale_node.argument_type(),
                        Some(&SteelArgumentType::from(ArgumentType::double(
                            f64::MIN,
                            f64::MAX,
                        )))
                    );
                }
            }
        }

        let modifier_paths: &[&[&str]] = &[
            &["as", "targets"],
            &["at", "targets"],
            &["positioned", "pos"],
            &["positioned", "as", "targets"],
            &["positioned", "over", "heightmap"],
            &["rotated", "rot"],
            &["rotated", "as", "targets"],
            &["facing", "pos"],
            &["facing", "entity", "targets", "anchor"],
            &["align", "axes"],
            &["anchored", "anchor"],
            &["in", "dimension"],
            &["summon", "entity"],
            &["on", "vehicle"],
            &["on", "controller"],
            &["on", "passengers"],
        ];
        for path in modifier_paths {
            let terminal = path
                .iter()
                .fold(execute, |parent, name| child(parent, name));
            let Some(terminal_node) = dispatcher.node(terminal) else {
                panic!("execute modifier terminal should exist");
            };
            assert_eq!(
                terminal_node.redirect(),
                Some(execute),
                "execute {} should redirect to the execute root",
                path.join(" ")
            );
        }

        let argument_types: &[(&[&str], SteelArgumentType)] = &[
            (&["as", "targets"], SteelArgumentType::entities()),
            (&["at", "targets"], SteelArgumentType::entities()),
            (&["positioned", "pos"], SteelArgumentType::vec3(true)),
            (
                &["positioned", "over", "heightmap"],
                SteelArgumentType::heightmap(),
            ),
            (&["rotated", "rot"], SteelArgumentType::rotation()),
            (&["facing", "pos"], SteelArgumentType::vec3(true)),
            (
                &["facing", "entity", "targets", "anchor"],
                SteelArgumentType::entity_anchor(),
            ),
            (&["align", "axes"], SteelArgumentType::swizzle()),
            (&["anchored", "anchor"], SteelArgumentType::entity_anchor()),
            (
                &["summon", "entity"],
                SteelArgumentType::summonable_entity(),
            ),
        ];
        for (path, expected) in argument_types {
            let argument = path
                .iter()
                .fold(execute, |parent, name| child(parent, name));
            assert_eq!(
                dispatcher
                    .node(argument)
                    .and_then(|node| node.argument_type()),
                Some(expected),
                "execute {} should use the expected argument parser",
                path.join(" ")
            );
        }
    }
}
