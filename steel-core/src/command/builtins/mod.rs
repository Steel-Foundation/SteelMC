//! Steel-owned built-in command declarations.

mod list;
mod seed;
mod stop;

use super::{
    brigadier::CommandDispatcher,
    execution::{CommandSource, SteelCommandRuntime},
    registration::{CommandDispatcherBuilder, CommandRegistrationError},
};

pub(crate) fn create_dispatcher()
-> Result<CommandDispatcher<CommandSource, SteelCommandRuntime>, CommandRegistrationError> {
    let mut builder = CommandDispatcherBuilder::new();
    builder.register(list::registration())?;
    builder.register(seed::registration())?;
    builder.register(stop::registration())?;
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::create_dispatcher;

    #[test]
    fn first_builtin_slice_has_the_expected_graph_shape() {
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

        assert_eq!(names, ["list", "seed", "stop"]);

        let Some(list) = roots.first().copied() else {
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
    }
}
