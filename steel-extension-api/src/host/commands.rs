//! Builds core command trees from validated native specifications.
use crate::{
    ApiResult, Command,
    command::{
        ArgumentKind, CommandNodeSpec, CommandSpec, NodeKind, ROOT_PARENT, SenderKind,
        api::SuggestionRequest,
    },
};
use stabby::{string::String as ApiString, vec::Vec as ApiVec};
use std::{collections::BTreeSet, sync::Arc};
use steel_core::{
    command::{
        CommandArgument, CommandContext, CommandError, CommandNode, CommandRegistration,
        CommandRegistry, CommandSuggestionContext,
    },
    permission::{PermissionExpr, PermissionKey, PermissionState},
};
use steel_utils::Identifier;

type ExecuteCallback = extern "C" fn(Command<'_>, u32, u64) -> ApiResult<i32>;
type SuggestCallback = extern "C" fn(SuggestionRequest<'_>, u32, u64) -> ApiVec<ApiString>;

/// Host data behind [`SuggestionRequest`].
pub(crate) struct SuggestionData<'a> {
    pub context: CommandSuggestionContext<'a>,
    pub remaining: String,
}

struct NodeData {
    parent: Option<usize>,
    kind: NodeKind,
    name: String,
    argument: ArgumentKind,
    min: f64,
    max: f64,
    executes: bool,
    permission: Option<PermissionExpr>,
    suggests: bool,
}

#[derive(Clone, Copy)]
struct Callbacks {
    execute: ExecuteCallback,
    suggest: SuggestCallback,
    token: u64,
}

/// Validates and converts the flat native node list.
fn validate(key: &Identifier, nodes: &[CommandNodeSpec]) -> Result<Vec<NodeData>, String> {
    let Some(root) = nodes.first() else {
        return Err("command tree needs a root node".into());
    };
    if root.parent != ROOT_PARENT || root.kind != NodeKind::Literal {
        return Err("the first command node must be the root literal".into());
    }
    if root.name.as_str() != key.path {
        return Err(format!(
            "root literal must match the command key path {}",
            key.path
        ));
    }
    let mut siblings = BTreeSet::new();
    let mut data = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let parent = if index == 0 {
            None
        } else {
            let parent = usize::try_from(node.parent).unwrap_or(usize::MAX);
            if parent >= index {
                return Err(format!("command node {index} must follow its parent"));
            }
            Some(parent)
        };
        let name = node.name.as_str();
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(format!("invalid command node name {name:?}"));
        }
        if !siblings.insert((parent, name.to_owned())) {
            return Err(format!("duplicate command node {name:?} under one parent"));
        }
        match (node.kind, node.argument) {
            (NodeKind::Literal, ArgumentKind::None) => {}
            (NodeKind::Literal, _) => return Err(format!("literal {name:?} has an argument type")),
            (NodeKind::Argument, ArgumentKind::None) => {
                return Err(format!("argument {name:?} has no argument type"));
            }
            (NodeKind::Argument, _) => {}
        }
        if node.min > node.max {
            return Err(format!("argument {name:?} has inverted bounds"));
        }
        let permission = if node.permission.as_str().is_empty() {
            None
        } else {
            let key = PermissionKey::parse(node.permission.as_str())
                .map_err(|error| format!("invalid permission on {name:?}: {error}"))?;
            Some(PermissionExpr::key(key))
        };
        data.push(NodeData {
            parent,
            kind: node.kind,
            name: name.to_owned(),
            argument: node.argument,
            min: node.min,
            max: node.max,
            executes: node.executes,
            permission,
            suggests: node.suggests && node.kind == NodeKind::Argument,
        });
    }
    Ok(data)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "bounds are clamped to the integer range before conversion"
)]
const fn int_bound(value: f64, low: i64, high: i64) -> i64 {
    if value.is_nan() {
        return low;
    }
    value.clamp(low as f64, high as f64) as i64
}

fn argument(node: &NodeData) -> CommandArgument {
    let i32_bounds = || {
        (
            i32::try_from(int_bound(node.min, i32::MIN.into(), i32::MAX.into()))
                .unwrap_or(i32::MIN),
            i32::try_from(int_bound(node.max, i32::MIN.into(), i32::MAX.into()))
                .unwrap_or(i32::MAX),
        )
    };
    match node.argument {
        ArgumentKind::Boolean => CommandArgument::boolean(),
        ArgumentKind::Integer => {
            let (min, max) = i32_bounds();
            CommandArgument::integer(min, max)
        }
        ArgumentKind::Long => CommandArgument::long(
            int_bound(node.min, i64::MIN, i64::MAX),
            int_bound(node.max, i64::MIN, i64::MAX),
        ),
        ArgumentKind::Double => CommandArgument::double(node.min, node.max),
        ArgumentKind::Word | ArgumentKind::None => CommandArgument::word(),
        ArgumentKind::String => CommandArgument::string(),
        ArgumentKind::Greedy => CommandArgument::greedy_string(),
        ArgumentKind::Player => CommandArgument::player(),
        ArgumentKind::Players => CommandArgument::players(),
        ArgumentKind::BlockPos => CommandArgument::block_pos(),
        ArgumentKind::BlockState => CommandArgument::block_state(),
        ArgumentKind::Item => CommandArgument::item_stack(),
        ArgumentKind::World => CommandArgument::world(),
    }
}

fn build(index: usize, nodes: &Arc<Vec<NodeData>>, callbacks: Callbacks) -> CommandNode {
    let data = &nodes[index];
    let mut node = match data.kind {
        NodeKind::Literal => CommandNode::literal(data.name.clone()),
        NodeKind::Argument => CommandNode::argument(data.name.clone(), argument(data)),
    };
    let node_index = u32::try_from(index).unwrap_or(u32::MAX);
    if data.executes {
        node = node.executes(move |context: &CommandContext<'_>| {
            let result: Result<i32, ApiString> =
                (callbacks.execute)(Command::from_host(context), node_index, callbacks.token)
                    .into();
            result.map_err(|error| CommandError::from(error.to_string()))
        });
    }
    if let Some(permission) = data.permission.clone() {
        node = node.requires(move |source| {
            source.player().is_none()
                || source.permission_state(&permission) == Some(PermissionState::Allow)
        });
    }
    if data.suggests {
        node = node.suggests_with(move |context, builder| {
            let request = SuggestionData {
                context,
                remaining: builder.remaining().to_owned(),
            };
            let suggestions = (callbacks.suggest)(
                SuggestionRequest::from_host(&request),
                node_index,
                callbacks.token,
            );
            let prefix = request.remaining.to_lowercase();
            for suggestion in &suggestions {
                if suggestion.as_str().to_lowercase().starts_with(&prefix) {
                    builder.suggest(suggestion.as_str());
                }
            }
        });
    }
    for child in (index + 1..nodes.len()).filter(|child| nodes[*child].parent == Some(index)) {
        node = node.then(build(child, nodes, callbacks));
    }
    node
}

pub(crate) fn register_tree(
    registry: &mut CommandRegistry,
    spec: &CommandSpec,
    execute: ExecuteCallback,
    suggest: SuggestCallback,
    token: u64,
) -> Result<(), String> {
    let key: Identifier = spec.key.as_str().parse().map_err(str::to_owned)?;
    let nodes = Arc::new(validate(&key, spec.nodes.as_slice())?);
    let callbacks = Callbacks {
        execute,
        suggest,
        token,
    };
    let mut registration = CommandRegistration::new(key, move || build(0, &nodes, callbacks));
    for alias in &spec.aliases {
        registration = registration.alias(alias.as_str());
    }
    if spec.default_access {
        registration = registration.default_access();
    }
    if !spec.permission.as_str().is_empty() {
        let key = PermissionKey::parse(spec.permission.as_str())
            .map_err(|error| format!("invalid command permission: {error}"))?;
        registration = registration.permission(PermissionExpr::key(key));
    }
    registry
        .register(registration)
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn sender(context: &CommandContext<'_>) -> SenderKind {
    let source = context.source();
    if source.player().is_some() {
        SenderKind::Player
    } else if source.entity().is_some() {
        SenderKind::Other
    } else {
        SenderKind::Console
    }
}

pub(crate) fn has_permission(context: &CommandContext<'_>, node: &str, default: bool) -> bool {
    let source = context.source();
    if source.player().is_none() {
        return true;
    }
    let Ok(key) = PermissionKey::parse(node) else {
        return false;
    };
    match source.permission_state(&PermissionExpr::key(key)) {
        Some(PermissionState::Allow) => true,
        Some(PermissionState::Deny) => false,
        None => default,
    }
}

/// Converts an argument read error into an ABI error string.
pub(crate) fn read_error(error: CommandError) -> ApiString {
    ApiString::from(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(parent: u32, kind: NodeKind, name: &str, argument: ArgumentKind) -> CommandNodeSpec {
        CommandNodeSpec {
            parent,
            kind,
            name: name.into(),
            argument,
            min: 0.0,
            max: 0.0,
            executes: true,
            permission: ApiString::default(),
            suggests: false,
        }
    }

    fn key() -> Identifier {
        Identifier::new_static("test", "home")
    }

    #[test]
    fn accepts_a_root_with_typed_children() {
        let nodes = [
            node(ROOT_PARENT, NodeKind::Literal, "home", ArgumentKind::None),
            node(0, NodeKind::Argument, "name", ArgumentKind::Word),
            node(0, NodeKind::Literal, "list", ArgumentKind::None),
        ];
        assert_eq!(validate(&key(), &nodes).map(|data| data.len()), Ok(3));
    }

    #[test]
    fn rejects_malformed_trees() {
        let wrong_root = [node(
            ROOT_PARENT,
            NodeKind::Literal,
            "other",
            ArgumentKind::None,
        )];
        assert!(validate(&key(), &wrong_root).is_err());
        let forward_parent = [
            node(ROOT_PARENT, NodeKind::Literal, "home", ArgumentKind::None),
            node(2, NodeKind::Literal, "a", ArgumentKind::None),
            node(0, NodeKind::Literal, "b", ArgumentKind::None),
        ];
        assert!(validate(&key(), &forward_parent).is_err());
        let duplicate = [
            node(ROOT_PARENT, NodeKind::Literal, "home", ArgumentKind::None),
            node(0, NodeKind::Literal, "a", ArgumentKind::None),
            node(0, NodeKind::Argument, "a", ArgumentKind::Word),
        ];
        assert!(validate(&key(), &duplicate).is_err());
        let untyped = [
            node(ROOT_PARENT, NodeKind::Literal, "home", ArgumentKind::None),
            node(0, NodeKind::Argument, "a", ArgumentKind::None),
        ];
        assert!(validate(&key(), &untyped).is_err());
    }

    #[test]
    fn same_name_under_different_parents_is_allowed() {
        let nodes = [
            node(ROOT_PARENT, NodeKind::Literal, "home", ArgumentKind::None),
            node(0, NodeKind::Literal, "a", ArgumentKind::None),
            node(0, NodeKind::Literal, "b", ArgumentKind::None),
            node(1, NodeKind::Argument, "x", ArgumentKind::Word),
            node(2, NodeKind::Argument, "x", ArgumentKind::Word),
        ];
        assert!(validate(&key(), &nodes).is_ok());
    }
}
