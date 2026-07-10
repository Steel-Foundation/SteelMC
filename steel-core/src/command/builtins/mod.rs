//! Steel-owned built-in command declarations.

mod difficulty;
mod domain;
pub(crate) mod gamemode;
mod gamerule;
mod kill;
mod list;
mod seed;
mod setworldspawn;
mod stop;
mod summon;
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
    builder.register(difficulty::registration())?;
    builder.register(domain::registration())?;
    builder.register(gamemode::registration()?)?;
    builder.register(gamerule::registration())?;
    builder.register(kill::registration())?;
    builder.register(list::registration())?;
    builder.register(seed::registration())?;
    builder.register(setworldspawn::registration())?;
    builder.register(stop::registration())?;
    builder.register(summon::registration())?;
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
                "difficulty",
                "domain",
                "gamemode",
                "gamerule",
                "kill",
                "list",
                "seed",
                "setworldspawn",
                "stop",
                "summon",
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
}
