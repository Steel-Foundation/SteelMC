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
mod seed;
mod setworldspawn;
mod stop;
mod summon;
mod teleport;
mod tick;
mod time;
mod weather;

pub(crate) use difficulty::player_can_change_difficulty;

use super::{
    brigadier::CommandDispatcher,
    execution::{CommandSource, SteelCommandRuntime},
    registration::{CommandDispatcherBuilder, CommandRegistrationError},
};

pub(crate) fn create_dispatcher()
-> Result<CommandDispatcher<CommandSource, SteelCommandRuntime>, CommandRegistrationError> {
    let mut builder = CommandDispatcherBuilder::new();
    builder.register(clear::registration())?;
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
    builder.register(seed::registration())?;
    builder.register(setworldspawn::registration())?;
    builder.register(stop::registration())?;
    builder.register(summon::registration())?;
    builder.register(teleport::registration())?;
    builder.register(tick::registration())?;
    builder.register(time::registration())?;
    builder.register(weather::registration())?;
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::create_dispatcher;
    use crate::command::execution::SteelArgumentType;
    use steel_registry::test_support::init_test_registry;

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
                "seed",
                "setworldspawn",
                "stop",
                "summon",
                "teleport",
                "tp",
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
        assert!(
            dispatcher
                .node(list)
                .is_some_and(|node| node.is_executable())
        );
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
                assert!(matches!(
                    duration.argument_type(),
                    Some(SteelArgumentType::Time { minimum: 1 })
                ));
                node.name()
            })
            .collect::<Vec<_>>();
        assert_eq!(weather_names, ["clear", "rain", "thunder"]);
    }

    #[test]
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
        assert_eq!(
            dispatcher.node(run).and_then(|node| node.redirect()),
            Some(dispatcher.root())
        );

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
            let targets = child(
                child(child(child(execute, "store"), store_kind), "score"),
                "targets",
            );
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
            &["summon", "entity"],
            &["on", "vehicle"],
            &["on", "controller"],
            &["on", "passengers"],
        ];
        for path in modifier_paths {
            let terminal = path
                .iter()
                .fold(execute, |parent, name| child(parent, name));
            assert_eq!(
                dispatcher.node(terminal).and_then(|node| node.redirect()),
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
