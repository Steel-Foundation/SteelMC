//! Steel permission administration under `/perms`.

mod config;

use std::{convert::Infallible, fmt};

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandPermissionSource, CommandResultSuspension, CommandResultSuspensionPoll,
        CommandSource, CommandSuspensionOrder, GameProfileArgument, SteelArgumentType,
        SteelCommandContext, SteelCommandRuntime, argument, literal,
    },
    registration::CommandRegistration,
};
use crate::command::missing_argument;
use crate::permission::{
    MODERATORS_COLLECTION, PermissionCollectionMember, PermissionContext, PermissionEntry,
    PermissionGroupRule, PermissionKey, PermissionMetadataEntry, PermissionMetadataExpression,
    PermissionMetadataValue, PermissionResolutionSource, PermissionRuleExpression, PermissionState,
    PermissionSubjectState,
};
use steel_utils::Identifier;
use text_components::TextComponent;
use tokio::{sync::oneshot, task::JoinHandle};

pub(super) const MANAGE_ALL_PERMISSION: &str = "steel:permission.manage.*";
pub(super) const GROUP_ALL_PERMISSION: &str = "steel:permission.group.*";
pub(super) const METADATA_PERMISSION: &str = "steel:permission.metadata";

/// Collection that receives `steel:command.perms` by default.
///
/// Change this to another vanilla collection, such as `ADMINS_COLLECTION`, to
/// restrict `/perms` to a higher level without changing its tests.
pub(super) const PERMS_COLLECTION: Identifier = MODERATORS_COLLECTION;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::from_steel("perms"), |_| command())
        .permission_collection(PERMS_COLLECTION)
        .subcommand_permission(["user", "info"])
        .subcommand_permission(["user", "allow"])
        .subcommand_permission(["user", "deny"])
        .subcommand_permission(["user", "unset"])
        .subcommand_permission(["user", "check"])
        .subcommand_permission(["user", "metadata", "set"])
        .subcommand_permission(["user", "metadata", "check"])
        .subcommand_permission(["user", "metadata", "unset"])
        .subcommand_permission(["user", "group", "add"])
        .subcommand_permission(["user", "group", "remove"])
        .subcommand_permission(["group", "create"])
        .subcommand_permission(["group", "info"])
        .subcommand_permission(["group", "delete"])
        .subcommand_permission(["group", "allow"])
        .subcommand_permission(["group", "deny"])
        .subcommand_permission(["group", "unset"])
        .subcommand_permission(["group", "priority"])
        .subcommand_permission(["group", "inherit", "list"])
        .subcommand_permission(["group", "inherit", "add"])
        .subcommand_permission(["group", "inherit", "remove"])
        .subcommand_permission(["group", "metadata", "set"])
        .subcommand_permission(["group", "metadata", "unset"])
        .subcommand_permission(["groups", "list"])
        .subcommand_permission(["groups", "default", "add"])
        .subcommand_permission(["groups", "default", "remove"])
        .subcommand_permission(["collection", "info"])
        .subcommand_permission(["collection", "export"])
        .subcommand_permission(["collection", "add"])
        .subcommand_permission(["collection", "remove"])
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("perms")
        .then(user_command())
        .then(group_command())
        .then(groups_command())
        .then(collection_command())
}

fn user_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("user").then(
        argument("targets", SteelArgumentType::game_profile())
            .then(literal("info").executes_suspended(user_info))
            .then(
                literal("allow").then(
                    argument("permission", SteelArgumentType::permission_rule())
                        .executes_suspended(user_allow),
                ),
            )
            .then(
                literal("deny").then(
                    argument("permission", SteelArgumentType::permission_rule())
                        .executes_suspended(user_deny),
                ),
            )
            .then(
                literal("unset").then(
                    argument("permission", SteelArgumentType::user_permission_rule())
                        .executes_suspended(user_unset),
                ),
            )
            .then(
                literal("check").then(
                    argument("permission", SteelArgumentType::permission_rule())
                        .executes_suspended(user_check),
                ),
            )
            .then(user_metadata_command())
            .then(
                literal("group")
                    .then(
                        literal("add").then(
                            argument("group", SteelArgumentType::permission_group(true))
                                .executes_suspended(user_group_add),
                        ),
                    )
                    .then(
                        literal("remove").then(
                            argument("group", SteelArgumentType::permission_group(true))
                                .executes_suspended(user_group_remove),
                        ),
                    ),
            ),
    )
}

fn user_metadata_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("metadata")
        .then(metadata_set_command(user_metadata_set))
        .then(
            literal("check").then(
                argument("metadata", SteelArgumentType::permission_metadata())
                    .executes_suspended(user_metadata_check),
            ),
        )
        .then(
            literal("unset").then(
                argument("metadata", SteelArgumentType::user_permission_metadata())
                    .executes_suspended(user_metadata_unset),
            ),
        )
}

fn group_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("group").then(
        argument("group", SteelArgumentType::permission_group(false))
            .then(literal("create").executes_suspended(group_create))
            .then(literal("info").executes_suspended(group_info))
            .then(literal("delete").executes_suspended(group_delete))
            .then(
                literal("allow").then(
                    argument("permission", SteelArgumentType::group_rule())
                        .executes_suspended(group_allow),
                ),
            )
            .then(
                literal("deny").then(
                    argument("permission", SteelArgumentType::group_rule())
                        .executes_suspended(group_deny),
                ),
            )
            .then(
                literal("unset").then(
                    argument("permission", SteelArgumentType::group_rule())
                        .executes_suspended(group_unset),
                ),
            )
            .then(
                literal("priority").then(
                    argument("priority", ArgumentType::integer(i32::MIN, i32::MAX))
                        .executes_suspended(group_priority),
                ),
            )
            .then(
                literal("inherit")
                    .then(literal("list").executes_suspended(group_inherit_list))
                    .then(
                        literal("add").then(
                            argument("parent", SteelArgumentType::permission_group(true))
                                .executes_suspended(group_inherit_add),
                        ),
                    )
                    .then(
                        literal("remove").then(
                            argument("parent", SteelArgumentType::permission_group(true))
                                .executes_suspended(group_inherit_remove),
                        ),
                    ),
            )
            .then(
                literal("metadata")
                    .then(metadata_set_command(group_metadata_set))
                    .then(
                        literal("unset").then(
                            argument("metadata", SteelArgumentType::group_permission_metadata())
                                .executes_suspended(group_metadata_unset),
                        ),
                    ),
            ),
    )
}

fn groups_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("groups")
        .then(literal("list").executes_suspended(groups_list))
        .then(
            literal("default")
                .then(
                    literal("add").then(
                        argument("group", SteelArgumentType::permission_group(true))
                            .executes_suspended(default_group_add),
                    ),
                )
                .then(
                    literal("remove").then(
                        argument("group", SteelArgumentType::permission_group(true))
                            .executes_suspended(default_group_remove),
                    ),
                ),
        )
}

fn collection_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("collection")
        .then(
            literal("export")
                .executes_suspended(collection_export_all)
                .then(
                    argument("collection", SteelArgumentType::permission_collection())
                        .executes_suspended(collection_export),
                ),
        )
        .then(
            argument("collection", SteelArgumentType::permission_collection())
                .then(literal("info").executes_suspended(collection_info))
                .then(
                    literal("add").then(
                        argument("member", SteelArgumentType::permission_collection_member())
                            .executes_suspended(collection_add),
                    ),
                )
                .then(
                    literal("remove").then(
                        argument("member", SteelArgumentType::permission_collection_member())
                            .executes_suspended(collection_remove),
                    ),
                ),
        )
}

fn metadata_set_command(
    executor: fn(
        &SteelCommandContext<CommandSource>,
    ) -> Result<PermsCommandSuspension, CommandSyntaxError>,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("set")
        .then(
            literal("int").then(
                argument("metadata_int_value", ArgumentType::long(i64::MIN, i64::MAX)).then(
                    argument("metadata", SteelArgumentType::permission_metadata())
                        .executes_suspended(executor),
                ),
            ),
        )
        .then(
            literal("bool").then(
                argument("metadata_bool_value", ArgumentType::bool()).then(
                    argument("metadata", SteelArgumentType::permission_metadata())
                        .executes_suspended(executor),
                ),
            ),
        )
        .then(
            literal("string").then(
                argument("metadata_string_value", ArgumentType::string()).then(
                    argument("metadata", SteelArgumentType::permission_metadata())
                        .executes_suspended(executor),
                ),
            ),
        )
}

#[derive(Clone)]
enum Operation {
    UserInfo(GameProfileArgument),
    UserPermission {
        targets: GameProfileArgument,
        expression: PermissionRuleExpression,
        state: Option<PermissionState>,
    },
    UserCheck {
        targets: GameProfileArgument,
        expression: PermissionRuleExpression,
    },
    UserMetadata {
        targets: GameProfileArgument,
        expression: PermissionMetadataExpression,
        value: Option<PermissionMetadataValue>,
    },
    UserMetadataCheck {
        targets: GameProfileArgument,
        expression: PermissionMetadataExpression,
    },
    UserGroup {
        targets: GameProfileArgument,
        group: String,
        add: bool,
    },
    GroupInfo(String),
    GroupCreate(String),
    GroupDelete(String),
    GroupPermission {
        group: String,
        rule: PermissionGroupRule,
        state: Option<PermissionState>,
    },
    GroupPriority {
        group: String,
        priority: i32,
    },
    GroupInheritanceList(String),
    GroupInheritance {
        group: String,
        parent: String,
        add: bool,
    },
    GroupMetadata {
        group: String,
        expression: PermissionMetadataExpression,
        value: Option<PermissionMetadataValue>,
    },
    GroupsList,
    DefaultGroup {
        group: String,
        add: bool,
    },
    CollectionInfo(Identifier),
    CollectionExport(Option<Identifier>),
    CollectionMember {
        collection: Identifier,
        member: PermissionCollectionMember,
        add: bool,
    },
}

impl Operation {
    const fn suspension_order(&self) -> CommandSuspensionOrder {
        match self {
            Self::UserInfo(_)
            | Self::UserCheck { .. }
            | Self::UserMetadataCheck { .. }
            | Self::GroupInfo(_)
            | Self::GroupInheritanceList(_)
            | Self::CollectionInfo(_)
            | Self::GroupsList => CommandSuspensionOrder::Source,
            Self::UserPermission { .. }
            | Self::UserMetadata { .. }
            | Self::UserGroup { .. }
            | Self::GroupCreate(_)
            | Self::GroupDelete(_)
            | Self::GroupPermission { .. }
            | Self::GroupPriority { .. }
            | Self::GroupInheritance { .. }
            | Self::GroupMetadata { .. }
            | Self::DefaultGroup { .. }
            | Self::CollectionExport(_)
            | Self::CollectionMember { .. } => CommandSuspensionOrder::Global,
        }
    }
}

struct OperationResult {
    result: i32,
    messages: Vec<TextComponent>,
}

fn user_info(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    start(context, Operation::UserInfo(targets(context)?))
}

fn user_allow(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    user_permission(context, Some(PermissionState::Allow))
}

fn user_deny(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    user_permission(context, Some(PermissionState::Deny))
}

fn user_unset(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    user_permission(context, None)
}

fn user_permission(
    context: &SteelCommandContext<CommandSource>,
    state: Option<PermissionState>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let expression = permission_expression(context)?;
    require_permission_management(context.source(), expression.key())?;
    start(
        context,
        Operation::UserPermission {
            targets: targets(context)?,
            expression,
            state,
        },
    )
}

fn user_check(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let expression = permission_expression(context)?;
    require_permission_management(context.source(), expression.key())?;
    start(
        context,
        Operation::UserCheck {
            targets: targets(context)?,
            expression,
        },
    )
}

fn user_metadata_set(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    user_metadata(context, Some(metadata_value(context)?))
}

fn user_metadata_unset(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    user_metadata(context, None)
}

fn user_metadata(
    context: &SteelCommandContext<CommandSource>,
    value: Option<PermissionMetadataValue>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    require_metadata_management(context.source())?;
    start(
        context,
        Operation::UserMetadata {
            targets: targets(context)?,
            expression: metadata_expression(context)?,
            value,
        },
    )
}

fn user_metadata_check(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    require_metadata_management(context.source())?;
    start(
        context,
        Operation::UserMetadataCheck {
            targets: targets(context)?,
            expression: metadata_expression(context)?,
        },
    )
}

fn user_group_add(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    user_group(context, true)
}

fn user_group_remove(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    user_group(context, false)
}

fn user_group(
    context: &SteelCommandContext<CommandSource>,
    add: bool,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = group_argument(context, "group")?;
    require_group_management(context.source(), &group)?;
    start(
        context,
        Operation::UserGroup {
            targets: targets(context)?,
            group,
            add,
        },
    )
}

fn group_info(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = managed_group(context)?;
    start(context, Operation::GroupInfo(group))
}

fn group_create(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = managed_group(context)?;
    start(context, Operation::GroupCreate(group))
}

fn group_delete(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = managed_group(context)?;
    start(context, Operation::GroupDelete(group))
}

fn group_allow(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    group_permission(context, Some(PermissionState::Allow))
}

fn group_deny(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    group_permission(context, Some(PermissionState::Deny))
}

fn group_unset(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    group_permission(context, None)
}

fn group_permission(
    context: &SteelCommandContext<CommandSource>,
    state: Option<PermissionState>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = managed_group(context)?;
    let rule = context.permission_group_rule("permission")?.clone();
    require_group_rule_management(context.source(), &rule)?;
    start(context, Operation::GroupPermission { group, rule, state })
}

fn group_priority(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = managed_group(context)?;
    let priority = context.integer("priority")?;
    start(context, Operation::GroupPriority { group, priority })
}

fn group_inherit_list(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = managed_group(context)?;
    start(context, Operation::GroupInheritanceList(group))
}

fn group_inherit_add(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    group_inheritance(context, true)
}

fn group_inherit_remove(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    group_inheritance(context, false)
}

fn group_inheritance(
    context: &SteelCommandContext<CommandSource>,
    add: bool,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = managed_group(context)?;
    let parent = group_argument(context, "parent")?;
    require_group_management(context.source(), &parent)?;
    start(context, Operation::GroupInheritance { group, parent, add })
}

fn group_metadata_set(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    group_metadata(context, Some(metadata_value(context)?))
}

fn group_metadata_unset(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    group_metadata(context, None)
}

fn group_metadata(
    context: &SteelCommandContext<CommandSource>,
    value: Option<PermissionMetadataValue>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    require_metadata_management(context.source())?;
    let group = managed_group(context)?;
    start(
        context,
        Operation::GroupMetadata {
            group,
            expression: metadata_expression(context)?,
            value,
        },
    )
}

fn groups_list(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    start(context, Operation::GroupsList)
}

fn collection_info(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let collection = context
        .permission_collection("collection")?
        .as_identifier()
        .clone();
    require_collection_management(context.source(), &collection)?;
    start(context, Operation::CollectionInfo(collection))
}

fn collection_export_all(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    start(context, Operation::CollectionExport(None))
}

fn collection_export(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let collection = context
        .permission_collection("collection")?
        .as_identifier()
        .clone();
    require_collection_management(context.source(), &collection)?;
    start(context, Operation::CollectionExport(Some(collection)))
}

fn collection_add(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    collection_member(context, true)
}

fn collection_remove(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    collection_member(context, false)
}

fn collection_member(
    context: &SteelCommandContext<CommandSource>,
    add: bool,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let collection = context
        .permission_collection("collection")?
        .as_identifier()
        .clone();
    require_collection_management(context.source(), &collection)?;
    let member = context.permission_collection_member("member")?.clone();
    start(
        context,
        Operation::CollectionMember {
            collection,
            member,
            add,
        },
    )
}

fn default_group_add(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    default_group(context, true)
}

fn default_group_remove(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    default_group(context, false)
}

fn default_group(
    context: &SteelCommandContext<CommandSource>,
    add: bool,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let group = group_argument(context, "group")?;
    require_group_management(context.source(), &group)?;
    start(context, Operation::DefaultGroup { group, add })
}

fn targets(
    context: &SteelCommandContext<CommandSource>,
) -> Result<GameProfileArgument, CommandSyntaxError> {
    context.game_profile_argument("targets").cloned()
}

fn permission_expression(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermissionRuleExpression, CommandSyntaxError> {
    context.permission_rule_expression("permission").cloned()
}

fn metadata_expression(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermissionMetadataExpression, CommandSyntaxError> {
    context.permission_metadata_expression("metadata").cloned()
}

fn metadata_value(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PermissionMetadataValue, CommandSyntaxError> {
    if let Ok(value) = context.long("metadata_int_value") {
        return Ok(PermissionMetadataValue::Integer(value));
    }
    if let Ok(value) = context.boolean("metadata_bool_value") {
        return Ok(PermissionMetadataValue::Bool(value));
    }
    if let Ok(value) = context.string("metadata_string_value") {
        return Ok(PermissionMetadataValue::String(value.to_owned()));
    }
    Err(missing_argument("metadata value"))
}

fn group_argument(
    context: &SteelCommandContext<CommandSource>,
    name: &str,
) -> Result<String, CommandSyntaxError> {
    context
        .permission_group(name)
        .map(|group| group.as_str().to_owned())
}

fn managed_group(
    context: &SteelCommandContext<CommandSource>,
) -> Result<String, CommandSyntaxError> {
    let group = group_argument(context, "group")?;
    require_group_management(context.source(), &group)?;
    Ok(group)
}

fn require_permission_management(
    source: &CommandSource,
    permission: &PermissionKey,
) -> Result<(), CommandSyntaxError> {
    require_pattern_permission(source, &permission.manage_authority())
}

fn require_group_rule_management(
    source: &CommandSource,
    rule: &PermissionGroupRule,
) -> Result<(), CommandSyntaxError> {
    if source.can_manage_group_rule(rule) {
        return Ok(());
    }
    Err(CommandSyntaxError::dynamic(format!(
        "Requires permission {}",
        group_rule_authority(rule)
    )))
}

fn require_collection_management(
    source: &CommandSource,
    collection: &Identifier,
) -> Result<(), CommandSyntaxError> {
    require_permission_management(source, &PermissionKey::from_identifier(collection))
}

fn require_group_management(source: &CommandSource, group: &str) -> Result<(), CommandSyntaxError> {
    let key = management_key(format!("steel:permission.group.{group}"))?;
    require_pattern_permission(source, &key)
}

fn require_metadata_management(source: &CommandSource) -> Result<(), CommandSyntaxError> {
    let key = management_key(METADATA_PERMISSION.to_owned())?;
    require_pattern_permission(source, &key)
}

fn management_key(value: String) -> Result<PermissionKey, CommandSyntaxError> {
    PermissionKey::parse(value.clone()).map_err(|error| {
        CommandSyntaxError::dynamic(format!("Invalid management permission '{value}': {error}"))
    })
}

fn require_pattern_permission(
    source: &CommandSource,
    key: &PermissionKey,
) -> Result<(), CommandSyntaxError> {
    if source.has_permission_pattern(key) {
        Ok(())
    } else {
        Err(CommandSyntaxError::dynamic(format!(
            "Requires permission {key}"
        )))
    }
}

fn group_rule_authority(rule: &PermissionGroupRule) -> PermissionKey {
    match rule {
        PermissionGroupRule::Expression(expression) => expression.key().manage_authority(),
        PermissionGroupRule::Collection(collection) => {
            PermissionKey::from_identifier(collection).manage_authority()
        }
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "suspended command callbacks use one fallible constructor signature"
)]
fn start(
    context: &SteelCommandContext<CommandSource>,
    operation: Operation,
) -> Result<PermsCommandSuspension, CommandSyntaxError> {
    let order = operation.suspension_order();
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_operation(&task_source, operation).await;
        let _ = sender.send(result);
    });
    Ok(PermsCommandSuspension {
        source,
        order,
        broadcast_to_admins: order == CommandSuspensionOrder::Global,
        receiver,
        task: Some(task),
    })
}

async fn run_operation(
    source: &CommandSource,
    operation: Operation,
) -> Result<OperationResult, CommandSyntaxError> {
    match operation {
        Operation::UserInfo(targets) => user_info_operation(source, targets).await,
        Operation::UserPermission {
            targets,
            expression,
            state,
        } => user_permission_operation(source, targets, expression, state).await,
        Operation::UserCheck {
            targets,
            expression,
        } => user_check_operation(source, targets, expression).await,
        Operation::UserMetadata {
            targets,
            expression,
            value,
        } => user_metadata_operation(source, targets, expression, value).await,
        Operation::UserMetadataCheck {
            targets,
            expression,
        } => user_metadata_check_operation(source, targets, expression).await,
        Operation::UserGroup {
            targets,
            group,
            add,
        } => user_group_operation(source, targets, group, add).await,
        Operation::GroupInfo(group) => group_info_operation(source, group),
        Operation::GroupCreate(group) => group_create_operation(source, group).await,
        Operation::GroupDelete(group) => group_delete_operation(source, group).await,
        Operation::GroupPermission { group, rule, state } => {
            group_permission_operation(source, group, rule, state).await
        }
        Operation::GroupPriority { group, priority } => {
            group_priority_operation(source, group, priority).await
        }
        Operation::GroupInheritanceList(group) => group_inheritance_list_operation(source, group),
        Operation::GroupInheritance { group, parent, add } => {
            group_inheritance_operation(source, group, parent, add).await
        }
        Operation::GroupMetadata {
            group,
            expression,
            value,
        } => group_metadata_operation(source, group, expression, value).await,
        Operation::GroupsList => Ok(groups_list_operation(source)),
        Operation::DefaultGroup { group, add } => default_group_operation(source, group, add).await,
        Operation::CollectionInfo(collection) => collection_info_operation(source, collection),
        Operation::CollectionExport(collection) => {
            collection_export_operation(source, collection).await
        }
        Operation::CollectionMember {
            collection,
            member,
            add,
        } => collection_member_operation(source, collection, member, add).await,
    }
}

async fn user_info_operation(
    source: &CommandSource,
    targets: GameProfileArgument,
) -> Result<OperationResult, CommandSyntaxError> {
    let targets = targets.resolve(source).await?;
    let show_metadata = has_dynamic_permission(source, METADATA_PERMISSION);
    let mut messages = Vec::new();
    for target in &targets {
        let state = source
            .server()
            .player_permission_state(target.uuid)
            .unwrap_or_default();
        let groups = state
            .groups()
            .iter()
            .filter(|group| can_manage_group(source, group))
            .cloned()
            .collect::<Vec<_>>();
        let rules = state
            .overrides()
            .entries()
            .iter()
            .filter(|entry| source.can_manage_permission(entry.key()))
            .map(|entry| {
                format!(
                    "{}={}",
                    PermissionRuleExpression::new(entry.key().clone(), entry.context().clone()),
                    state_name(entry.state())
                )
            })
            .collect::<Vec<_>>();
        let metadata = if show_metadata {
            state
                .metadata_overrides()
                .entries()
                .iter()
                .map(|entry| {
                    format!(
                        "{}={}",
                        PermissionMetadataExpression::new(
                            entry.key().clone(),
                            entry.context().clone()
                        ),
                        entry.value()
                    )
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        messages.push(TextComponent::plain(format!(
            "{}: groups [{}], rules [{}], metadata [{}]",
            target.name,
            groups.join(", "),
            rules.join(", "),
            metadata.join(", ")
        )));
    }
    Ok(OperationResult {
        result: count(targets.len()),
        messages,
    })
}

async fn user_permission_operation(
    source: &CommandSource,
    targets: GameProfileArgument,
    expression: PermissionRuleExpression,
    state: Option<PermissionState>,
) -> Result<OperationResult, CommandSyntaxError> {
    let targets = targets.resolve(source).await?;
    let mut changed = 0;
    let mut messages = Vec::new();
    for target in targets {
        let edit_expression = expression.clone();
        let result = source
            .server()
            .try_update_player_permissions(target.uuid, move |subject| {
                let (groups, mut overrides, metadata) = subject.into_parts();
                let exact = overrides.entries().iter().filter(|entry| {
                    entry.key() == edit_expression.key()
                        && entry.context() == edit_expression.context()
                });
                let exact = exact.map(PermissionEntry::state).collect::<Vec<_>>();
                let did_change = match state {
                    Some(state) => {
                        let changed = exact.as_slice() != [state];
                        overrides.set_in(
                            edit_expression.key().clone(),
                            edit_expression.context().clone(),
                            state,
                        );
                        changed
                    }
                    None => overrides.unset_in(edit_expression.key(), edit_expression.context()),
                };
                Ok::<_, Infallible>((
                    PermissionSubjectState::new_with_metadata(groups, overrides, metadata),
                    did_change,
                ))
            })
            .await
            .map_err(dynamic_error)?;
        if result.1 {
            changed += 1;
        }
        messages.push(TextComponent::plain(format!(
            "{}: {} {}",
            target.name,
            state.map_or("unset", state_name),
            expression
        )));
    }
    Ok(OperationResult {
        result: changed,
        messages,
    })
}

async fn user_check_operation(
    source: &CommandSource,
    targets: GameProfileArgument,
    expression: PermissionRuleExpression,
) -> Result<OperationResult, CommandSyntaxError> {
    let targets = targets.resolve(source).await?;
    let context =
        PermissionContext::from_rule_context(expression.context()).map_err(dynamic_error)?;
    let mut messages = Vec::new();
    for target in &targets {
        let state = source
            .server()
            .player_permission_state(target.uuid)
            .unwrap_or_default();
        let effective = source
            .server()
            .permission_groups
            .effective_permissions(state.groups(), state.overrides());
        let resolution = effective.resolve_key_in_detailed(expression.key(), &context);
        let detail = resolution.as_ref().map_or_else(
            || "unset".to_owned(),
            |resolution| {
                format!(
                    "{} via {} ({})",
                    state_name(resolution.state()),
                    resolution_source(resolution.source()),
                    PermissionRuleExpression::new(
                        resolution.key().clone(),
                        resolution.context().clone()
                    )
                )
            },
        );
        messages.push(TextComponent::plain(format!(
            "{}: {} -> {detail}",
            target.name, expression
        )));
    }
    Ok(OperationResult {
        result: count(targets.len()),
        messages,
    })
}

async fn user_metadata_operation(
    source: &CommandSource,
    targets: GameProfileArgument,
    expression: PermissionMetadataExpression,
    value: Option<PermissionMetadataValue>,
) -> Result<OperationResult, CommandSyntaxError> {
    let targets = targets.resolve(source).await?;
    let mut changed = 0;
    let mut messages = Vec::new();
    for target in targets {
        let edit_expression = expression.clone();
        let edit_value = value.clone();
        let result = source
            .server()
            .try_update_player_permissions(target.uuid, move |subject| {
                let (groups, overrides, mut metadata) = subject.into_parts();
                let previous = metadata.entries().iter().find(|entry| {
                    entry.key() == edit_expression.key()
                        && entry.context() == edit_expression.context()
                });
                let did_change = match edit_value {
                    Some(value) => {
                        let changed = previous.map(PermissionMetadataEntry::value) != Some(&value);
                        metadata.set_in(
                            edit_expression.key().clone(),
                            edit_expression.context().clone(),
                            value,
                        );
                        changed
                    }
                    None => metadata.unset_in(edit_expression.key(), edit_expression.context()),
                };
                Ok::<_, Infallible>((
                    PermissionSubjectState::new_with_metadata(groups, overrides, metadata),
                    did_change,
                ))
            })
            .await
            .map_err(dynamic_error)?;
        if result.1 {
            changed += 1;
        }
        messages.push(TextComponent::plain(format!(
            "{}: {} {}",
            target.name,
            value
                .as_ref()
                .map_or_else(|| "unset".to_owned(), |value| format!("set {value}")),
            expression
        )));
    }
    Ok(OperationResult {
        result: changed,
        messages,
    })
}

async fn user_metadata_check_operation(
    source: &CommandSource,
    targets: GameProfileArgument,
    expression: PermissionMetadataExpression,
) -> Result<OperationResult, CommandSyntaxError> {
    let targets = targets.resolve(source).await?;
    let context =
        PermissionContext::from_rule_context(expression.context()).map_err(dynamic_error)?;
    let mut messages = Vec::new();
    for target in &targets {
        let state = source
            .server()
            .player_permission_state(target.uuid)
            .unwrap_or_default();
        let effective = source
            .server()
            .permission_groups
            .effective_metadata(state.groups(), state.metadata_overrides());
        let resolution = effective.resolve_in_detailed(expression.key(), &context);
        let detail = resolution.as_ref().map_or_else(
            || "unset".to_owned(),
            |resolution| {
                format!(
                    "{} via {} ({})",
                    resolution.value(),
                    resolution_source(resolution.source()),
                    PermissionMetadataExpression::new(
                        resolution.key().clone(),
                        resolution.context().clone()
                    )
                )
            },
        );
        messages.push(TextComponent::plain(format!(
            "{}: {} -> {detail}",
            target.name, expression
        )));
    }
    Ok(OperationResult {
        result: count(targets.len()),
        messages,
    })
}

async fn user_group_operation(
    source: &CommandSource,
    targets: GameProfileArgument,
    group: String,
    add: bool,
) -> Result<OperationResult, CommandSyntaxError> {
    let targets = targets.resolve(source).await?;
    let mut changed = 0;
    let mut messages = Vec::new();
    for target in targets {
        let edit_group = group.clone();
        let result = source
            .server()
            .try_update_player_permissions(target.uuid, move |subject| {
                let (mut groups, overrides, metadata) = subject.into_parts();
                let present = groups.iter().any(|assigned| assigned == &edit_group);
                let did_change = if add {
                    if !present {
                        groups.push(edit_group);
                    }
                    !present
                } else {
                    groups.retain(|assigned| assigned != &edit_group);
                    present
                };
                Ok::<_, Infallible>((
                    PermissionSubjectState::new_with_metadata(groups, overrides, metadata),
                    did_change,
                ))
            })
            .await
            .map_err(dynamic_error)?;
        if result.1 {
            changed += 1;
        }
        messages.push(TextComponent::plain(format!(
            "{}: {} group {group}",
            target.name,
            if add { "added" } else { "removed" }
        )));
    }
    Ok(OperationResult {
        result: changed,
        messages,
    })
}

fn group_info_operation(
    source: &CommandSource,
    group: String,
) -> Result<OperationResult, CommandSyntaxError> {
    let config = source.server().permission_groups.group_config_snapshot();
    let Some(group_config) = config.groups.get(&group) else {
        return Err(CommandSyntaxError::dynamic(format!(
            "Unknown permission group '{group}'"
        )));
    };
    let allow = group_config
        .allow
        .iter()
        .filter(|expression| manageable_expression(source, expression))
        .cloned()
        .collect::<Vec<_>>();
    let deny = group_config
        .deny
        .iter()
        .filter(|expression| manageable_expression(source, expression))
        .cloned()
        .collect::<Vec<_>>();
    let metadata = if has_dynamic_permission(source, METADATA_PERMISSION) {
        group_config
            .metadata
            .iter()
            .map(|rule| format!("{}={}", rule.key, rule.value))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let inherits = group_config
        .inherits
        .iter()
        .filter(|parent| can_manage_group(source, parent))
        .cloned()
        .collect::<Vec<_>>();
    Ok(OperationResult {
        result: 1,
        messages: vec![TextComponent::plain(format!(
            "Group '{group}': priority {}, inherits [{}], allow [{}], deny [{}], metadata [{}]",
            group_config.priority,
            inherits.join(", "),
            allow.join(", "),
            deny.join(", "),
            metadata.join(", ")
        ))],
    })
}

async fn group_create_operation(
    source: &CommandSource,
    group: String,
) -> Result<OperationResult, CommandSyntaxError> {
    let edit_group = group.clone();
    let changed = source
        .server()
        .try_update_permission_groups(move |config| config::create_group(config, &edit_group))
        .await
        .map_err(dynamic_error)?;
    Ok(change_result(changed, format!("Created group '{group}'")))
}

async fn group_delete_operation(
    source: &CommandSource,
    group: String,
) -> Result<OperationResult, CommandSyntaxError> {
    let edit_group = group.clone();
    let changed = source
        .server()
        .try_update_permission_groups(move |config| config::delete_group(config, &edit_group))
        .await
        .map_err(dynamic_error)?;
    Ok(change_result(changed, format!("Deleted group '{group}'")))
}

async fn group_permission_operation(
    source: &CommandSource,
    group: String,
    rule: PermissionGroupRule,
    state: Option<PermissionState>,
) -> Result<OperationResult, CommandSyntaxError> {
    let edit_group = group.clone();
    let edit_rule = rule.clone();
    let changed = source
        .server()
        .try_update_permission_groups(move |config| {
            config::set_permission(config, &edit_group, &edit_rule, state)
        })
        .await
        .map_err(dynamic_error)?;
    Ok(change_result(
        changed,
        format!(
            "Group '{group}': {} {rule}",
            state.map_or("unset", state_name)
        ),
    ))
}

async fn group_priority_operation(
    source: &CommandSource,
    group: String,
    priority: i32,
) -> Result<OperationResult, CommandSyntaxError> {
    let edit_group = group.clone();
    let changed = source
        .server()
        .try_update_permission_groups(move |config| {
            config::set_priority(config, &edit_group, priority)
        })
        .await
        .map_err(dynamic_error)?;
    Ok(change_result(
        changed,
        format!("Group '{group}': priority {priority}"),
    ))
}

fn group_inheritance_list_operation(
    source: &CommandSource,
    group: String,
) -> Result<OperationResult, CommandSyntaxError> {
    let config = source.server().permission_groups.group_config_snapshot();
    let Some(group_config) = config.groups.get(&group) else {
        return Err(CommandSyntaxError::dynamic(format!(
            "Unknown permission group '{group}'"
        )));
    };
    let parents = group_config
        .inherits
        .iter()
        .filter(|parent| can_manage_group(source, parent))
        .cloned()
        .collect::<Vec<_>>();
    Ok(OperationResult {
        result: count(parents.len()),
        messages: vec![TextComponent::plain(format!(
            "Group '{group}' inherits [{}]",
            parents.join(", ")
        ))],
    })
}

async fn group_inheritance_operation(
    source: &CommandSource,
    group: String,
    parent: String,
    add: bool,
) -> Result<OperationResult, CommandSyntaxError> {
    let edit_group = group.clone();
    let edit_parent = parent.clone();
    let changed = source
        .server()
        .try_update_permission_groups(move |config| {
            config::set_inheritance(config, &edit_group, &edit_parent, add)
        })
        .await
        .map_err(dynamic_error)?;
    Ok(change_result(
        changed,
        format!(
            "Group '{group}': {} inheritance '{parent}'",
            if add { "added" } else { "removed" }
        ),
    ))
}

async fn group_metadata_operation(
    source: &CommandSource,
    group: String,
    expression: PermissionMetadataExpression,
    value: Option<PermissionMetadataValue>,
) -> Result<OperationResult, CommandSyntaxError> {
    let edit_group = group.clone();
    let edit_expression = expression.clone();
    let edit_value = value.clone();
    let changed = source
        .server()
        .try_update_permission_groups(move |config| {
            config::set_metadata(config, &edit_group, &edit_expression, edit_value)
        })
        .await
        .map_err(dynamic_error)?;
    Ok(change_result(
        changed,
        format!(
            "Group '{group}': {} {expression}",
            value.map_or_else(|| "unset".to_owned(), |value| format!("set {value}"))
        ),
    ))
}

fn groups_list_operation(source: &CommandSource) -> OperationResult {
    let config = source.server().permission_groups.group_config_snapshot();
    let defaults = config
        .default_groups
        .iter()
        .filter(|group| can_manage_group(source, group))
        .cloned()
        .collect::<Vec<_>>();
    let groups = config
        .groups
        .keys()
        .filter(|group| can_manage_group(source, group))
        .cloned()
        .collect::<Vec<_>>();
    OperationResult {
        result: count(groups.len()),
        messages: vec![TextComponent::plain(format!(
            "Permission groups: defaults [{}], groups [{}]",
            defaults.join(", "),
            groups.join(", ")
        ))],
    }
}

fn collection_info_operation(
    source: &CommandSource,
    collection: Identifier,
) -> Result<OperationResult, CommandSyntaxError> {
    let collections = source.server().permission_groups.collections_snapshot();
    let Some(entry) = collections.get(&collection) else {
        return Err(CommandSyntaxError::dynamic(format!(
            "Unknown permission collection '{collection}'"
        )));
    };
    let members = entry
        .members()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    Ok(OperationResult {
        result: count(members.len()),
        messages: vec![TextComponent::plain(format!(
            "Collection '{collection}': [{}]",
            members.join(", ")
        ))],
    })
}

async fn collection_export_operation(
    source: &CommandSource,
    collection: Option<Identifier>,
) -> Result<OperationResult, CommandSyntaxError> {
    let collections = source.server().permission_groups.collections_snapshot();
    let ids = match &collection {
        Some(id) => {
            if !collections.contains(id) {
                return Err(CommandSyntaxError::dynamic(format!(
                    "Unknown permission collection '{id}'"
                )));
            }
            vec![id.clone()]
        }
        None => collections.ids().cloned().collect::<Vec<_>>(),
    };
    if ids.is_empty() {
        return Ok(change_result(
            false,
            "No permission collections to export".to_owned(),
        ));
    }

    let mut exported = Vec::with_capacity(ids.len());
    for id in &ids {
        let Some(entry) = collections.get(id) else {
            continue;
        };
        let members = entry
            .members()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        exported.push((id.clone(), members));
    }

    let before = source
        .server()
        .permission_groups
        .collections_config_snapshot();
    source
        .server()
        .update_permission_collections(move |config| {
            for (id, members) in exported {
                config
                    .collections
                    .entry(id.namespace.to_string())
                    .or_default()
                    .insert(id.path.to_string(), members);
            }
        })
        .await
        .map_err(dynamic_error)?;
    let changed = before
        != source
            .server()
            .permission_groups
            .collections_config_snapshot();

    let target = collection.map_or_else(
        || format!("{} permission collections", ids.len()),
        |id| format!("permission collection '{id}'"),
    );
    let mut result = change_result(changed, format!("Exported {target} to collections.toml"));
    result.messages.push(TextComponent::plain(
        "Exported collections are stored in collections.toml and are no longer updated automatically",
    ));
    Ok(result)
}

async fn collection_member_operation(
    source: &CommandSource,
    collection: Identifier,
    member: PermissionCollectionMember,
    add: bool,
) -> Result<OperationResult, CommandSyntaxError> {
    let collections = source.server().permission_groups.collections_snapshot();
    let Some(entry) = collections.get(&collection) else {
        return Err(CommandSyntaxError::dynamic(format!(
            "Unknown permission collection '{collection}'"
        )));
    };

    let mut members = entry.members().to_vec();
    if !config::edit_collection_members(&mut members, &member, add) {
        return Ok(change_result(
            false,
            format!(
                "Collection '{collection}': member '{member}' {}",
                if add {
                    "already present"
                } else {
                    "not present"
                }
            ),
        ));
    }

    let stored_id = collection.clone();
    source
        .server()
        .update_permission_collections(move |config| {
            config::set_collection_members(config, &stored_id, &members);
        })
        .await
        .map_err(dynamic_error)?;

    let mut result = change_result(
        true,
        format!(
            "Collection '{collection}': {} member '{member}'",
            if add { "added" } else { "removed" }
        ),
    );
    result.messages.push(TextComponent::plain(
        "Edited collections are stored in collections.toml and are no longer updated automatically",
    ));
    Ok(result)
}

async fn default_group_operation(
    source: &CommandSource,
    group: String,
    add: bool,
) -> Result<OperationResult, CommandSyntaxError> {
    let edit_group = group.clone();
    let changed = source
        .server()
        .try_update_permission_groups(move |config| {
            config::set_default_group(config, &edit_group, add)
        })
        .await
        .map_err(dynamic_error)?;
    Ok(change_result(
        changed,
        format!(
            "Group '{group}': {} default assignment",
            if add { "added" } else { "removed" }
        ),
    ))
}

fn manageable_expression(source: &CommandSource, expression: &str) -> bool {
    source.can_manage_rule(expression)
}

fn can_manage_group(source: &CommandSource, group: &str) -> bool {
    has_dynamic_permission(source, &format!("steel:permission.group.{group}"))
}

fn has_dynamic_permission(source: &CommandSource, value: &str) -> bool {
    PermissionKey::parse(value).is_ok_and(|key| source.has_permission_pattern(&key))
}

const fn state_name(state: PermissionState) -> &'static str {
    match state {
        PermissionState::Allow => "allow",
        PermissionState::Deny => "deny",
    }
}

fn resolution_source(source: &PermissionResolutionSource) -> String {
    match source {
        PermissionResolutionSource::Group { name, priority } => {
            format!("group {name} priority {priority}")
        }
        PermissionResolutionSource::Subject => "subject override".to_owned(),
    }
}

fn change_result(changed: bool, message: String) -> OperationResult {
    OperationResult {
        result: i32::from(changed),
        messages: vec![TextComponent::plain(if changed {
            message
        } else {
            format!("No change: {message}")
        })],
    }
}

fn count(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

fn dynamic_error(error: impl fmt::Display) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(error.to_string())
}

struct PermsCommandSuspension {
    source: CommandSource,
    order: CommandSuspensionOrder,
    broadcast_to_admins: bool,
    receiver: oneshot::Receiver<Result<OperationResult, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for PermsCommandSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        self.order
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|result| {
                    for message in &result.messages {
                        self.source.send_success(message, self.broadcast_to_admins);
                    }
                    result.result
                }))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "perms command task ended without a result",
                )))
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::{
        GROUP_ALL_PERMISSION, MANAGE_ALL_PERMISSION, METADATA_PERMISSION, PERMS_COLLECTION,
    };
    use crate::command::{
        CommandRegistry,
        brigadier::{CommandDispatcher, NodeId},
        builtins::{create_dispatcher, create_registered_dispatcher},
        execution::{CommandSource, SteelCommandRuntime},
    };
    use crate::permission::{PermissionCollection, PermissionCollectionMember, PermissionKey};
    use steel_utils::Identifier;

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(child) = dispatcher.children(parent).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == name)
            })
        }) else {
            panic!("missing command node '{name}'");
        };
        child
    }

    #[test]
    fn perms_exposes_the_management_surface_without_old_aliases() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in dispatcher should build");
        };
        let roots = dispatcher.children(dispatcher.root());
        let Some(roots) = roots else {
            panic!("dispatcher root should exist");
        };
        assert!(!roots.iter().any(|root| {
            dispatcher
                .node(*root)
                .is_some_and(|node| matches!(node.name(), "steelperms" | "sp"))
        }));

        let perms = child(&dispatcher, dispatcher.root(), "perms");
        let user = child(&dispatcher, perms, "user");
        let targets = child(&dispatcher, user, "targets");
        for name in [
            "info", "allow", "deny", "unset", "check", "metadata", "group",
        ] {
            child(&dispatcher, targets, name);
        }
        let group = child(&dispatcher, perms, "group");
        let group_name = child(&dispatcher, group, "group");
        for name in [
            "create", "info", "delete", "allow", "deny", "unset", "priority", "inherit", "metadata",
        ] {
            child(&dispatcher, group_name, name);
        }
        let groups = child(&dispatcher, perms, "groups");
        child(&dispatcher, groups, "list");
        child(&dispatcher, groups, "default");

        let collection = child(&dispatcher, perms, "collection");
        child(&dispatcher, collection, "export");
        let collection_name = child(&dispatcher, collection, "collection");
        for name in ["info", "add", "remove"] {
            child(&dispatcher, collection_name, name);
        }
    }

    #[test]
    fn perms_discovery_contains_static_admin_and_granular_command_permissions() {
        init_vanilla_registry();
        let Ok(registered) = create_registered_dispatcher(CommandRegistry::new()) else {
            panic!("built-in dispatcher should build");
        };
        let permissions = registered
            .permissions
            .iter()
            .map(PermissionKey::as_str)
            .collect::<Vec<_>>();

        for expected in [
            MANAGE_ALL_PERMISSION,
            GROUP_ALL_PERMISSION,
            METADATA_PERMISSION,
            "steel:command.perms.user.allow",
            "steel:command.perms.user.metadata.set",
            "steel:command.perms.group.inherit.add",
            "steel:command.perms.collection.add",
        ] {
            assert!(permissions.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn perms_command_is_declared_in_its_default_collection() {
        init_vanilla_registry();
        let registered = match create_registered_dispatcher(CommandRegistry::new()) {
            Ok(registered) => registered,
            Err(error) => panic!("built-in dispatcher should build: {error}"),
        };
        let declared = registered.declared_collections();
        let members = declared
            .get(&PERMS_COLLECTION)
            .map(PermissionCollection::members);
        let command =
            PermissionCollectionMember::Permission(Identifier::from_steel("command.perms"));
        let manage = PermissionCollectionMember::Permission(Identifier::from_steel(
            "permission.manage.steel.command.perms",
        ));
        assert!(
            members
                .is_some_and(|members| { members.contains(&command) && members.contains(&manage) })
        );
    }
}
