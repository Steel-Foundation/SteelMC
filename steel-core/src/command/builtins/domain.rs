//! Steel domain switch command.

use std::sync::Arc;

use steel_registry::{
    data_components::vanilla_components::{CUSTOM_NAME, ENCHANTMENT_GLINT_OVERRIDE},
    vanilla_dimension_types, vanilla_items, vanilla_menu_types,
};
use steel_utils::Identifier;
use text_components::TextComponent;

use crate::{inventory::prelude::*, server::Server, world::World};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::from_steel("domain"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("domain")
        .executes(|ctx: &SteelCommandContext<CommandSource>| {
            let Some(player) = ctx.source().player() else {
                return Err(CommandSyntaxError::dynamic(
                    "you cannot use this command from the console",
                ));
            };
            let server = Arc::clone(ctx.source().server());
            let player = Arc::clone(player);
            let menu_player = Arc::clone(&player);

            player.open_menu("Domains", move |context| {
                domain_menu(context.container_id, menu_player, context.world, &server)
            });

            Ok(1)
        })
        .then(argument("world", SteelArgumentType::world()).executes(switch_world))
}

fn switch_world(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let Some(player) = source.player() else {
        return Err(CommandSyntaxError::dynamic(
            "This command can only be used by a player",
        ));
    };
    let world = context.world_argument("world")?;
    let world = world.resolve(source)?;
    source
        .server()
        .queue_player_world_selection(Arc::clone(player), Arc::clone(&world))
        .map_err(CommandSyntaxError::dynamic)?;

    source.send_success(
        &TextComponent::plain(format!("Switching to world {}", world.key)),
        true,
    );
    Ok(1)
}

fn domain_menu(
    container_id: u8,
    player: Arc<Player>,
    current_world: &Arc<World>,
    server: &Arc<Server>,
) -> Menu {
    let mut b = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X6, container_id);

    let domain_names: Vec<String> = server
        .worlds
        .domain_names()
        .map(ToOwned::to_owned)
        .collect();

    let map: Vec<(Section, Vec<Arc<World>>)> = b.grid(6, |g| {
        g.paint_all(ItemStack::empty());
        g.paint(
            Rect::cols(..).rows(0),
            &vanilla_items::GRAY_STAINED_GLASS_PANE,
        );
        g.paint(
            Rect::cols(..).rows(5),
            &vanilla_items::GRAY_STAINED_GLASS_PANE,
        );
        g.paint(
            Rect::cols(8).rows(..),
            &vanilla_items::GRAY_STAINED_GLASS_PANE,
        );
        g.paint(
            Rect::cols(0).rows(1..5),
            &vanilla_items::GRAY_STAINED_GLASS_PANE,
        );

        // TODO: Add pagination for domains and worlds instead of truncating to the grid capacity.
        domain_names
            .iter()
            .take(4)
            .enumerate()
            .map(|(i, domain_name)| {
                g.subgrid(Rect::cols(..8).rows(i + 1), |g| {
                    g.paint_all(ItemStack::empty());

                    let mut sign = ItemStack::new(&vanilla_items::OAK_SIGN);
                    sign.set(CUSTOM_NAME, domain_name.clone().into());

                    g.paint(Rect::cell(0, 0), sign);

                    let worlds = server.worlds.worlds_in_domain(domain_name);

                    let icons: Vec<ItemStack> =
                        worlds.iter().map(|w| icon(w, current_world)).collect();

                    let len = icons.len();

                    let container = SimpleContainer::from_items(icons).into_shared();

                    (
                        g.place(Rect::cols(1..(len + 1).min(6)).rows(0), container)
                            .display()
                            .section(),
                        worlds,
                    )
                })
            })
            .collect()
    });
    b.player_inventory(&player.inventory);

    b.build(DomainMenuKind {
        map,
        server: server.clone(),
        player,
    })
}

fn icon(world: &Arc<World>, current_world: &Arc<World>) -> ItemStack {
    let item = match world.dimension_type {
        b if b == &vanilla_dimension_types::OVERWORLD
            || b == &vanilla_dimension_types::OVERWORLD_CAVES =>
        {
            &vanilla_items::GRASS_BLOCK
        }
        b if b == &vanilla_dimension_types::THE_NETHER => &vanilla_items::NETHERRACK,
        b if b == &vanilla_dimension_types::THE_END => &vanilla_items::END_STONE,
        _ => &vanilla_items::BEDROCK,
    };
    let mut icon = ItemStack::new(item);
    icon.set(CUSTOM_NAME, world.key.path.to_string().into());
    if world.key == current_world.key {
        icon.set(ENCHANTMENT_GLINT_OVERRIDE, true);
    }
    icon
}

struct DomainMenuKind {
    map: Vec<(Section, Vec<Arc<World>>)>,
    server: Arc<Server>,
    player: Arc<Player>,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for DomainMenuKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/domain");
}

impl MenuKind for DomainMenuKind {
    fn on_slot_clicked(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        click: Click,
        _player: &Player,
    ) -> ClickOutcome {
        let Some(index) = click.slot() else {
            return ClickOutcome::Fallthrough;
        };

        let Some((section, worlds)) = self
            .map
            .iter()
            .find(|(section, _worlds)| section.contains(index))
        else {
            return ClickOutcome::Fallthrough;
        };

        if worlds.is_empty() {
            return ClickOutcome::Consume;
        }

        let Some(world) = worlds.get(index - section.start()) else {
            return ClickOutcome::Consume;
        };

        if !matches!(click, Click::Pickup { .. }) {
            return ClickOutcome::Consume;
        }

        if let Err(error) = self
            .server
            .queue_player_world_selection(Arc::clone(&self.player), Arc::clone(world))
        {
            tracing::debug!(%error, target_world = %world.key, "domain menu selection was rejected");
        }
        self.player.close_container();
        ClickOutcome::Consume
    }
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };
    use steel_registry::init_vanilla_registry;

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child {name} should exist");
        };
        child
    }

    #[test]
    fn domain_graph_uses_the_loaded_world_argument() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let root = child(&dispatcher, dispatcher.root(), "domain");
        let world = child(&dispatcher, root, "world");
        assert_eq!(
            dispatcher.node(world).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::world())
        );
        let Some(world) = dispatcher.node(world) else {
            panic!("world argument should exist");
        };
        assert!(world.is_executable());
    }
}
