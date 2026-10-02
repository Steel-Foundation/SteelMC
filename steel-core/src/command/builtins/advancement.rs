use crate::command::brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError};
use crate::command::execution::{
    CommandSource, SteelArgumentType, SteelCommandRuntime, argument, literal,
};
use crate::command::registration::CommandRegistration;
use crate::player::Player;
use crate::player::advancement::PlayerAdvancement;
use std::sync::Arc;
use steel_registry::REGISTRY;
use steel_registry::advancement::Advancement;
use steel_registry::advancement::registry::{AdvancementNode, AdvancementRef};
use steel_utils::Identifier;
use text_components::TextComponent;
use text_components::translation::Translation;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("advancement"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("advancement")
        .then(
            literal("grant").then(
                argument("targets", SteelArgumentType::players()).then(
                    literal("only")
                        .then(
                            argument("advancement", SteelArgumentType::advancement()).executes(
                                |c| {
                                    perform_and_show(
                                        c.source()?,
                                        &c.players("targets")?,
                                        Action::Grant,
                                        &get_advancements(
                                            c.advancement("advancement")?,
                                            Mode::Only,
                                        ),
                                    )
                                },
                            ),
                        )
                        .then(
                            argument("criterion", ArgumentType::greedy_string())
                                .suggests(|c, b| {
                                    c.argument("advancement")?
                                        .criteria
                                        .keySet()
                                        .foreach(|name| b.suggest(name))
                                })
                                .executes(|c| {
                                    perform_criterion(
                                        c.source()?,
                                        c.players("targets")?,
                                        Action::Grant,
                                        c.advancement("advancement")?,
                                        c.string("criterion")?,
                                    )
                                }),
                        ),
                ),
            ),
        )
        .then(literal("from").then(
            argument("advancement", SteelArgumentType::advancement()).executes(|c| {
                perform_and_show(
                    c.getSource()?,
                    c.players("targets")?,
                    Action::Grant,
                    get_advancements(c.advancement("advancement")?, Mode::From)?,
                )
            }),
        ))
        .then(literal("until").then(
            argument("advancement", SteelArgumentType::advancement()).executes(|c| {
                perform_and_show(
                    c.source()?,
                    c.players("targets")?,
                    Action::Grant,
                    get_advancements(c.advancement("advancement"), Mode::Until)?,
                )
            }),
        ))
        .then(literal("through").then(
            argument("advancement", SteelArgumentType::advancement()).executes(|c| {
                perform_and_show(
                    c.source()?,
                    c.players("targets")?,
                    Action::Grant,
                    get_advancements(c.advancement("advancement"), Mode::Through)?,
                )
            }),
        ))
        .then(literal("everything").executes(|c| {
            perform(
                c.source()?,
                c.players("targets")?,
                Action::Grant,
                c.getSource()
                    .getServer()
                    .getAdvancements()
                    .getAllAdvancements()?,
                false,
            )
        }))
}

#[derive(Clone, Copy)]
pub enum Action {
    Grant,
    Revoke,
}

impl Action {
    /// inner function that directly take the locked [`PlayerAdvancement`]
    fn perform_single_inner(
        self,
        player: &Player,
        guard: &mut PlayerAdvancement,
        advancement: AdvancementRef,
    ) -> bool {
        let progress = guard.progress.get_mut_or_start_progress(advancement);
        match self {
            Self::Grant => {
                if progress.is_done() {
                    return false;
                }
                let criteria: Vec<Arc<str>> = progress.get_remaining_criteria().collect();
                for criterion in criteria {
                    guard.award(player, advancement, &criterion);
                }
                true
            }
            Self::Revoke => {
                if !progress.has_progress() {
                    return false;
                }
                let criteria: Vec<Arc<str>> = progress.get_completed_criteria().collect();
                for criterion in criteria {
                    guard.revoke(advancement, &criterion);
                }
                true
            }
        }
    }

    fn perform(
        self,
        player: &Arc<Player>,
        advancements: &[AdvancementRef],
        show_advancement: bool,
    ) -> i32 {
        if !show_advancement {
            let mut guard = player.advancements.lock();
            guard.flush_dirty(player, true);
        }
        let mut guard = player.advancements.lock();
        let count = advancements
            .iter()
            .filter(|advancement| self.perform_single_inner(player, &mut guard, advancement))
            .count() as i32;
        if !show_advancement {
            let mut guard = player.advancements.lock();
            guard.flush_dirty(player, false);
        }
        count
    }

    fn perform_criterion(
        self,
        player: &Arc<Player>,
        advancement: &'static Advancement,
        criterion: &str,
    ) -> bool {
        let mut guard = player.advancements.lock();
        match self {
            Self::Grant => guard.award(player, advancement, criterion),
            Self::Revoke => guard.revoke(advancement, criterion),
        }
    }

    /// return the corresponding key of the action
    const fn get_key(&self) -> &str {
        match self {
            Self::Grant => "commands.advancement.grant",
            Self::Revoke => "commands.advancement.revoke",
        }
    }
}

#[derive(Clone, Copy)]
#[allow(unused)]
enum Mode {
    Only,
    Through,
    From,
    Until,
    Everything,
}

impl Mode {
    const fn parents(self) -> bool {
        match self {
            Self::Only | Self::From => false,
            Self::Through | Self::Until | Self::Everything => true,
        }
    }

    const fn children(self) -> bool {
        match self {
            Self::Only | Self::Until => false,
            Self::Through | Self::From | Self::Everything => true,
        }
    }
}

fn get_advancements(target: AdvancementRef, mode: Mode) -> Vec<AdvancementRef> {
    let tree = REGISTRY.advancements;
    let target_node = tree.by_key(&target.key);
    target_node.map_or_else(
        || vec![target],
        |target_node| {
            let mut advancements = Vec::new();
            if mode.parents() {
                let mut parent = target_node.parent;
                while let Some(parent_id) = parent {
                    let current_node = &tree.adv_nodes[parent_id];
                    advancements.push(current_node.value);
                    parent = current_node.parent;
                }
            }
            advancements.push(target);
            if mode.children() {
                add_children(target_node, &mut advancements);
            }
            advancements
        },
    )
}

fn add_children(parent: &AdvancementNode, output: &mut Vec<&Advancement>) {
    for child in &parent.children {
        let node = &REGISTRY.advancements.adv_nodes[*child];
        output.push(node.value);
        add_children(node, output);
    }
}

#[inline]
fn perform_and_show(
    context: Arc<CommandSource>,
    players: &[Arc<Player>],
    action: Action,
    advancements: &[AdvancementRef],
) -> Result<i32, CommandSyntaxError> {
    perform(context, players, action, advancements, true)
}

fn perform(
    context: Arc<CommandSource>,
    targets: &[Arc<Player>],
    action: Action,
    advancements: &[AdvancementRef],
    show_advancement: bool,
) -> Result<i32, CommandSyntaxError> {
    let mut i = 0;
    for player in targets {
        i += action.perform(player, advancements, show_advancement);
    }
    if i == 0 {
        return if let [first_advancement] = advancements[..] {
            if let [first_player] = targets {
                Err(match action {
                    Action::Grant => &ERROR_GRANT_ONE_TO_ONE,
                    Action::Revoke => &ERROR_REVOKE_ONE_TO_ONE,
                }
                .create_without_context_args_slice(&[
                    first_advancement.name(),
                    first_player.get_display_name(),
                ]))
            } else {
                Err(match action {
                    Action::Grant => &ERROR_GRANT_ONE_TO_MANY,
                    Action::Revoke => &ERROR_REVOKE_ONE_TO_MANY,
                }
                .create_without_context_args_slice(&[
                    first_advancement.name(),
                    targets.len().to_string(),
                ]))
            }
        } else if let [first_player] = targets {
            Err(match action {
                Action::Grant => &ERROR_GRANT_MANY_TO_ONE,
                Action::Revoke => &ERROR_REVOKE_MANY_TO_ONE,
            }
            .create_without_context_args_slice(&[
                advancements.len().to_string(),
                first_player.get_display_name(),
            ]))
        } else {
            Err(match action {
                Action::Grant => &ERROR_GRANT_MANY_TO_MANY,
                Action::Revoke => &ERROR_REVOKE_MANY_TO_MANY,
            }
            .create_without_context_args_slice(&[
                advancements.len().to_string(),
                targets.len().to_string(),
            ]))
        };
    }
    let translate = if let [first_advancement] = advancements[..] {
        if let [first_player] = targets {
            TextComponent::translated(
                Translation(format!("{}.one.to.one.success", action.get_key()))
                    .message([first_advancement.name(), first_player.get_display_name()]),
            )
        } else {
            TextComponent::translated(
                Translation(format!("{}.one.to.many.success", action.get_key()))
                    .message([first_advancement.name(), targets.len().to_string()]),
            )
        }
    } else if let [first] = targets {
        TextComponent::translated(
            format!("{}.many.to.one.success", action.get_key()),
            [advancements.len().to_string(), first.get_display_name()],
        )
    } else {
        TextComponent::translated(
            format!("{}.many.to.many.success", action.get_key()),
            [advancements.len().to_string(), targets.len().to_string()],
        )
    };
    context.send_feedback(translate, true);
    Ok(i)
}

pub fn perform_criterion(
    context: &CommandSource,
    targets: &[Arc<Player>],
    action: Action,
    advancement: &'static Advancement,
    criterion: &str,
) -> Result<i32, CommandSyntaxError> {
    if !advancement.criteria.contains(&criterion) {
        return Err(
            ERROR_CRITERION_NOT_FOUND.create_without_context_args_slice(&[
                advancement.name(),
                TextComponent::text(criterion.to_owned()),
            ]),
        );
    }

    let count = targets
        .iter()
        .map(|player| action.perform_criterion(player, advancement, criterion))
        .filter(|&success| success)
        .count() as i32;

    if count == 0 {
        if let [first_player] = targets {
            Err(match action {
                Action::Grant => &ERROR_GRANT_CRITERION_TO_ONE_FAILURE,
                Action::Revoke => &ERROR_REVOKE_CRITERION_TO_ONE_FAILURE,
            }
            .create_without_context_args_slice(&[
                TextComponent::text(criterion.to_owned()),
                advancement.name(),
                first_player.get_display_name(),
            ]))
        } else {
            Err(match action {
                Action::Grant => &ERROR_GRANT_CRITERION_TO_MANY_FAILURE,
                Action::Revoke => &ERROR_REVOKE_CRITERION_TO_MANY_FAILURE,
            }
            .create_without_context_args_slice(&[
                TextComponent::text(criterion.to_owned()),
                advancement.name(),
                TextComponent::text(targets.len().to_string()),
            ]))
        }
    } else {
        let translate = if let [first_player] = targets {
            TextComponent::translate(
                format!("{}.criterion.to.one.success", action.get_key()),
                [
                    TextComponent::text(criterion.to_owned()),
                    advancement.name(),
                    first_player.get_display_name(),
                ],
            )
        } else {
            TextComponent::translate(
                format!("{}.criterion.to.many.success", action.get_key()),
                [
                    TextComponent::text(criterion.to_owned()),
                    advancement.name(),
                    TextComponent::text(count.to_string()),
                ],
            )
        };
        context.send_feedback(translate, true);
        Ok(count)
    }
}
