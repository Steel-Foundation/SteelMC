use crate::command::brigadier::{
    ArgumentType, CommandNodeBuilder, CommandSyntaxError, SuggestionsBuilder,
};
use crate::command::execution::{
    CommandSource, SteelArgumentType, SteelCommandRuntime, SteelSuggestionContext, argument,
    literal, suggest_list,
};
use crate::command::registration::CommandRegistration;
use crate::entity::Entity;
use crate::player::Player;
use crate::player::advancement::PlayerAdvancement;
use std::sync::Arc;
use steel_registry::REGISTRY;
use steel_registry::advancement::Advancement;
use steel_registry::advancement::registry::{AdvancementNode, AdvancementRef};
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("advancement"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    macro_rules! build_action {
        ($name:expr, $action:expr) => {
            literal($name)
                .then(argument("targets", SteelArgumentType::players())
                .then(literal("only")
                    .then(
                        argument("advancement", SteelArgumentType::advancement())
                        .executes(
                            |c| {
                                perform_and_show(
                                    c.source(),
                                    &c.players("targets")?,
                                    $action,
                                    &get_advancements(
                                        c.advancement("advancement")?,
                                        Mode::Only,
                                    ),
                                )
                            },
                        )
                        .then(
                            argument("criterion", ArgumentType::greedy_string())
                            .suggests(
                                |c: &SteelSuggestionContext<'_, CommandSource>,
                                b: &mut SuggestionsBuilder<'_>| {
                                    if let Ok(advancement) = c.advancement("advancement")  {
                                        suggest_list(b, advancement.criteria.keys());
                                    };
                                },
                            )
                            .executes(|c| {
                                perform_criterion(
                                    c.source(),
                                    &c.players("targets")?,
                                    $action,
                                    c.advancement("advancement")?,
                                    c.string("criterion")?,
                                )
                            }),
                        ),
                    ),
                )
                .then(literal("from").then(
                    argument("advancement", SteelArgumentType::advancement()).executes(|c| {
                        perform_and_show(
                            c.source(),
                            &c.players("targets")?,
                            $action,
                            &get_advancements(c.advancement("advancement")?, Mode::From),
                        )
                    }),
                ))
                .then(literal("until").then(
                    argument("advancement", SteelArgumentType::advancement()).executes(|c| {
                        perform_and_show(
                            c.source(),
                            &c.players("targets")?,
                            $action,
                            &get_advancements(c.advancement("advancement")?, Mode::Until),
                        )
                    }),
                ))
                .then(literal("through").then(
                    argument("advancement", SteelArgumentType::advancement()).executes(|c| {
                        perform_and_show(
                            c.source(),
                            &c.players("targets")?,
                            $action,
                            &get_advancements(c.advancement("advancement")?, Mode::Through),
                        )
                    }),
                ))
                .then(literal("everything").executes(|c| {
                    perform(
                        c.source(),
                        &c.players("targets")?,
                        $action,
                        &REGISTRY.advancements.advancements,
                        false,
                    )
                })))
        };
    }
    literal("advancement")
        .then(build_action!("grant", Action::Grant))
        .then(build_action!("revoke", Action::Revoke))
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
                let criteria: Vec<String> = progress
                    .get_remaining_criteria()
                    .map(str::to_owned)
                    .collect();
                for criterion in criteria {
                    guard.award(player, advancement, &criterion);
                }
                true
            }
            Self::Revoke => {
                if !progress.has_progress() {
                    return false;
                }
                let criteria: Vec<String> = progress
                    .get_completed_criteria()
                    .map(str::to_owned)
                    .collect();
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
        let mut guard = player.advancements.lock();
        if !show_advancement {
            guard.flush_dirty(player, true);
        }
        let count = advancements
            .iter()
            .filter(|advancement| self.perform_single_inner(player, &mut guard, advancement))
            .count() as i32;
        if !show_advancement {
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
}

#[derive(Clone, Copy)]
enum Mode {
    Only,
    Through,
    From,
    Until,
    #[expect(unused, reason = "to match vanilla")]
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
    let tree = &REGISTRY.advancements;
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
    context: &CommandSource,
    players: &[Arc<Player>],
    action: Action,
    advancements: &[AdvancementRef],
) -> Result<i32, CommandSyntaxError> {
    perform(context, players, action, advancements, true)
}

fn perform(
    context: &CommandSource,
    targets: &[Arc<Player>],
    action: Action,
    advancements: &[AdvancementRef],
    show_advancement: bool,
) -> Result<i32, CommandSyntaxError> {
    let mut player_count = 0;
    for player in targets {
        player_count += action.perform(player, advancements, show_advancement);
    }
    if player_count == 0 {
        return if let [first_advancement] = advancements[..] {
            if let [first_player] = targets {
                Err(CommandSyntaxError::dynamic(
                    match action {
                        Action::Grant => {
                            &translations::COMMANDS_ADVANCEMENT_GRANT_ONE_TO_ONE_FAILURE
                        }
                        Action::Revoke => {
                            &translations::COMMANDS_ADVANCEMENT_REVOKE_ONE_TO_ONE_FAILURE
                        }
                    }
                    .message([first_advancement.name(), first_player.display_name()]),
                ))
            } else {
                Err(CommandSyntaxError::dynamic(
                    match action {
                        Action::Grant => {
                            &translations::COMMANDS_ADVANCEMENT_GRANT_ONE_TO_MANY_FAILURE
                        }
                        Action::Revoke => {
                            &translations::COMMANDS_ADVANCEMENT_REVOKE_ONE_TO_MANY_FAILURE
                        }
                    }
                    .message([first_advancement.name(), targets.len().to_string().into()]),
                ))
            }
        } else if let [first_player] = targets {
            Err(CommandSyntaxError::dynamic(
                match action {
                    Action::Grant => &translations::COMMANDS_ADVANCEMENT_GRANT_MANY_TO_ONE_FAILURE,
                    Action::Revoke => {
                        &translations::COMMANDS_ADVANCEMENT_REVOKE_MANY_TO_ONE_FAILURE
                    }
                }
                .message([
                    advancements.len().to_string().into(),
                    first_player.display_name(),
                ]),
            ))
        } else {
            Err(CommandSyntaxError::dynamic(
                match action {
                    Action::Grant => &translations::COMMANDS_ADVANCEMENT_GRANT_MANY_TO_MANY_FAILURE,
                    Action::Revoke => {
                        &translations::COMMANDS_ADVANCEMENT_REVOKE_MANY_TO_MANY_FAILURE
                    }
                }
                .message([
                    TextComponent::from(advancements.len().to_string()),
                    TextComponent::from(targets.len().to_string()),
                ]),
            ))
        };
    }
    let translate = if let [first_advancement] = advancements[..] {
        if let [first_player] = targets {
            match action {
                Action::Grant => &translations::COMMANDS_ADVANCEMENT_GRANT_ONE_TO_ONE_SUCCESS,
                Action::Revoke => &translations::COMMANDS_ADVANCEMENT_REVOKE_ONE_TO_ONE_SUCCESS,
            }
            .message([first_advancement.name(), first_player.display_name()])
            .component()
        } else {
            match action {
                Action::Grant => &translations::COMMANDS_ADVANCEMENT_GRANT_ONE_TO_MANY_SUCCESS,
                Action::Revoke => &translations::COMMANDS_ADVANCEMENT_REVOKE_ONE_TO_MANY_SUCCESS,
            }
            .message([first_advancement.name(), player_count.to_string().into()])
            .component()
        }
    } else if let [first_player] = targets {
        match action {
            Action::Grant => &translations::COMMANDS_ADVANCEMENT_GRANT_MANY_TO_ONE_SUCCESS,
            Action::Revoke => &translations::COMMANDS_ADVANCEMENT_REVOKE_MANY_TO_ONE_SUCCESS,
        }
        .message([
            advancements.len().to_string().into(),
            first_player.display_name(),
        ])
        .component()
    } else {
        match action {
            Action::Grant => &translations::COMMANDS_ADVANCEMENT_GRANT_MANY_TO_MANY_SUCCESS,
            Action::Revoke => &translations::COMMANDS_ADVANCEMENT_REVOKE_MANY_TO_MANY_SUCCESS,
        }
        .message([
            TextComponent::from(advancements.len().to_string()),
            TextComponent::from(player_count.to_string()),
        ])
        .component()
    };
    context.send_success(&translate, true);
    Ok(player_count)
}

pub fn perform_criterion(
    context: &CommandSource,
    targets: &[Arc<Player>],
    action: Action,
    advancement: AdvancementRef,
    criterion: &str,
) -> Result<i32, CommandSyntaxError> {
    if !advancement.criteria.contains_key(criterion) {
        return Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_ADVANCEMENT_CRITERION_NOT_FOUND
                .message([advancement.name(), criterion.to_owned().into()]),
        ));
    }

    let player_count = targets
        .iter()
        .map(|player| action.perform_criterion(player, advancement, criterion))
        .filter(|&success| success)
        .count() as i32;

    if player_count == 0 {
        if let [first_player] = targets {
            Err(CommandSyntaxError::dynamic(
                match action {
                    Action::Grant => {
                        &translations::COMMANDS_ADVANCEMENT_GRANT_CRITERION_TO_ONE_FAILURE
                    }
                    Action::Revoke => {
                        &translations::COMMANDS_ADVANCEMENT_REVOKE_CRITERION_TO_ONE_FAILURE
                    }
                }
                .message([
                    criterion.to_owned().into(),
                    advancement.name(),
                    first_player.display_name(),
                ]),
            ))
        } else {
            Err(CommandSyntaxError::dynamic(
                match action {
                    Action::Grant => {
                        &translations::COMMANDS_ADVANCEMENT_GRANT_CRITERION_TO_MANY_FAILURE
                    }
                    Action::Revoke => {
                        &translations::COMMANDS_ADVANCEMENT_REVOKE_CRITERION_TO_MANY_FAILURE
                    }
                }
                .message([
                    criterion.to_owned().into(),
                    advancement.name(),
                    targets.len().to_string().into(),
                ]),
            ))
        }
    } else {
        let translate = if let [first_player] = targets {
            match action {
                Action::Grant => &translations::COMMANDS_ADVANCEMENT_GRANT_CRITERION_TO_ONE_SUCCESS,
                Action::Revoke => {
                    &translations::COMMANDS_ADVANCEMENT_REVOKE_CRITERION_TO_ONE_SUCCESS
                }
            }
            .message([
                criterion.to_owned().into(),
                advancement.name(),
                first_player.display_name(),
            ])
            .component()
        } else {
            match action {
                Action::Grant => {
                    &translations::COMMANDS_ADVANCEMENT_GRANT_CRITERION_TO_MANY_SUCCESS
                }
                Action::Revoke => {
                    &translations::COMMANDS_ADVANCEMENT_REVOKE_CRITERION_TO_MANY_SUCCESS
                }
            }
            .message([
                criterion.to_owned().into(),
                advancement.name(),
                player_count.to_string().into(),
            ])
            .component()
        };
        context.send_success(&translate, true);
        Ok(player_count)
    }
}
