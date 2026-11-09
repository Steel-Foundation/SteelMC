//! Dispatcher-owned command node arena.

use std::ops::Range;
#[cfg(test)]
use super::CommandContext;
use super::{
    BrigadierRuntime, CommandArgumentParser, CommandNodeBuilder, CommandRedirectTarget,
    CommandRuntime, CommandSyntaxError, CommandSyntaxErrorKind, ContextChain, NodeId, NodeKind,
    ParseError, ParseResults, ParsedCommandContext, RegistrationError, RegistrationErrorKind,
    StringRange, StringReader, SuggestionError, Suggestions, SuggestionsBuilder,
    node::{CommandNode, CommandNodeData, UnregisteredCommandNode},
};
use crate::command::brigadier::node::ArgumentData;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_DISPATCHER_ID: AtomicU64 = AtomicU64::new(1);

/// Owns a stable arena of command nodes.
pub(crate) struct CommandDispatcher<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    id: u64,
    nodes: Vec<CommandNode<S, R>>,
}

impl<S, R> CommandDispatcher<S, R>
where
    R: CommandRuntime<S>,
{
    /// Creates an empty dispatcher containing only its root node.
    pub(crate) fn new() -> Self {
        Self {
            id: NEXT_DISPATCHER_ID.fetch_add(1, Ordering::Relaxed),
            nodes: vec![CommandNode::root()],
        }
    }

    /// Returns the stable root node ID.
    pub(crate) const fn root(&self) -> NodeId {
        NodeId::new(self.id, 0)
    }

    /// Registers and merges a literal command tree.
    pub(crate) fn register(
        &mut self,
        builder: CommandNodeBuilder<S, R>,
    ) -> Result<NodeId, RegistrationError> {
        let mut node = builder.normalize()?;
        if node.kind() != NodeKind::Literal {
            return Err(RegistrationError::new(RegistrationErrorKind::ArgumentRoot));
        }

        self.validate_redirects(&node)?;
        let command_root = self
            .find_child(self.root(), node.name())
            .unwrap_or_else(|| NodeId::new(self.id, self.nodes.len()));
        node.resolve_command_root(command_root);
        self.validate_merge(self.root(), &node)?;
        Ok(self.apply_merge(self.root(), node))
    }

    /// Parses `input` into the best Brigadier command branch.
    pub(crate) fn parse<'input>(
        &self,
        input: &'input str,
        source: S,
    ) -> ParseResults<'input, S, R> {
        self.parse_reader(StringReader::new(input), source)
    }

    /// Parses from an existing reader while preserving its current cursor.
    pub(crate) fn parse_reader<'input>(
        &self,
        reader: StringReader<'input>,
        source: S,
    ) -> ParseResults<'input, S, R> {
        let context = ParsedCommandContext::new(Arc::new(source), self.root(), reader.cursor());
        self.parse_nodes(self.root(), reader, context)
    }

    /// Returns completions for the end of a parsed command input.
    pub(crate) fn completion_suggestions(
        &self,
        parse: &ParseResults<'_, S, R>,
    ) -> Result<Suggestions, SuggestionError> {
        let input = parse.reader().input();
        let cursor = parse.reader().total_length();
        let Some(context) = parse.context().find_suggestion_context(cursor) else {
            return Ok(Suggestions::empty());
        };
        let Some(children) = self.children(context.parent) else {
            return Ok(Suggestions::empty());
        };

        let mut candidate_sets = Vec::with_capacity(children.len());
        for child_id in children {
            let child = &self.nodes[child_id.index];
            // Steel filters internal command completions as an authorization
            // boundary; Brigadier itself exposes nodes regardless of canUse.
            if !child.requirement.allows(parse.context().source()) {
                continue;
            }

            let mut builder = SuggestionsBuilder::new(input, context.start.min(cursor))?;
            match &child.data {
                CommandNodeData::Root => {
                    unreachable!("the command graph never stores a root node as a child")
                }
                CommandNodeData::Literal(literal) => {
                    if literal
                        .to_lowercase()
                        .starts_with(builder.remaining_lowercase())
                    {
                        builder.suggest(literal.as_ref());
                    }
                }
                CommandNodeData::Argument(_, argument_data) => {
                    let argument_context = context.context.argument_suggestion_context();
                    argument_data.list_suggestions(&argument_context, &mut builder);
                }
            }
            candidate_sets.push(builder.build()?);
        }

        Suggestions::merge(input, candidate_sets)
    }

    /// Validates a parse and turns its redirect contexts into executable stages.
    pub(crate) fn context_chain(
        &self,
        parse: ParseResults<'_, S, R>,
    ) -> Result<ContextChain<S, R>, CommandSyntaxError> {
        let (context, reader, mut errors) = parse.into_parts();
        if reader.can_read() {
            if errors.len() == 1 {
                return Err(errors.remove(0).into_error());
            }
            let kind = if context.range().is_empty() {
                CommandSyntaxErrorKind::UnknownCommand
            } else {
                CommandSyntaxErrorKind::UnknownArgument
            };
            return Err(reader.error(kind));
        }
        if context.root().dispatcher != self.id {
            return Err(reader.error(CommandSyntaxErrorKind::UnknownCommand));
        }

        let input: Arc<str> = Arc::from(reader.input());
        let context = context.build(input);
        ContextChain::try_flatten(context)
            .ok_or_else(|| reader.error(CommandSyntaxErrorKind::UnknownCommand))
    }

    /// Returns a node if the ID belongs to this dispatcher.
    pub(crate) fn node(&self, id: NodeId) -> Option<&CommandNode<S, R>> {
        if id.dispatcher != self.id {
            return None;
        }
        self.nodes.get(id.index)
    }

    /// Returns a node's children in registration order.
    pub(crate) fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.node(id).map(|node| node.children.as_slice())
    }

    /// Returns the number of allocated nodes, including the root.
    pub(crate) const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn validate_merge(
        &self,
        parent: NodeId,
        incoming: &UnregisteredCommandNode<S, R>,
    ) -> Result<(), RegistrationError> {
        let Some(existing_id) = self.find_child(parent, incoming.name()) else {
            return Ok(());
        };
        let existing = &self.nodes[existing_id.index];
        existing.validate_compatible(incoming)?;
        for child in &incoming.children {
            self.validate_merge(existing_id, child)?;
        }
        Ok(())
    }

    fn validate_redirects(
        &self,
        node: &UnregisteredCommandNode<S, R>,
    ) -> Result<(), RegistrationError> {
        if let Some(redirect) = &node.redirect
            && let CommandRedirectTarget::Node(target) = redirect.target
            && self.node(target).is_none()
        {
            return Err(RegistrationError::new(
                RegistrationErrorKind::InvalidRedirectTarget { target },
            ));
        }
        for child in &node.children {
            self.validate_redirects(child)?;
        }
        Ok(())
    }

    fn apply_merge(
        &mut self,
        parent: NodeId,
        mut incoming: UnregisteredCommandNode<S, R>,
    ) -> NodeId {
        if let Some(existing_id) = self.find_child(parent, incoming.name()) {
            if incoming.executor.is_some() {
                self.nodes[existing_id.index].executor = incoming.executor.take();
            }
            for child in incoming.children {
                self.apply_merge(existing_id, child);
            }
            return existing_id;
        }

        let node_id = NodeId::new(self.id, self.nodes.len());
        let children = incoming.children;
        self.nodes.push(CommandNode {
            data: incoming.data,
            children: Vec::new(),
            executor: incoming.executor,
            requirement: incoming.requirement,
            execution_requirement: incoming.execution_requirement,
            redirect: incoming.redirect,
        });
        self.nodes[parent.index].children.push(node_id);
        for child in children {
            self.apply_merge(node_id, child);
        }
        node_id
    }

    fn find_child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        let parent = self.node(parent)?;
        parent.children.iter().copied().find(|child| {
            self.nodes
                .get(child.index)
                .is_some_and(|node| node.name() == name)
        })
    }

    fn parse_nodes<'input>(
        &self,
        parent: NodeId,
        original_reader: StringReader<'input>,
        context_so_far: ParsedCommandContext<S, R>,
    ) -> ParseResults<'input, S, R> {
        let mut errors = Vec::new();
        let mut potentials = Vec::new();

        for child_id in self.relevant_nodes(parent, &original_reader) {
            let child = &self.nodes[child_id.index];
            if !child.requirement.allows(context_so_far.source()) {
                continue;
            }

            let mut context = context_so_far.branch();
            let mut reader = original_reader.clone();
            if let Err(error) = self.parse_node(child_id, &mut reader, &mut context) {
                errors.push(ParseError::new(child_id, error));
                continue;
            }
            if reader.can_read() && reader.peek() != Some(' ') {
                errors.push(ParseError::new(
                    child_id,
                    reader.error(CommandSyntaxErrorKind::ExpectedArgumentSeparator),
                ));
                continue;
            }

            let executor = child
                .executor
                .as_ref()
                .filter(|_| child.execution_requirement.allows(context.source()))
                .cloned();
            context.set_executor(executor);
            let redirect = child.redirect();
            let required_remaining = if redirect.is_some() { 1 } else { 2 };
            if reader.can_read_length(required_remaining) {
                reader.skip();
                if let Some(target) = redirect {
                    let child_context = ParsedCommandContext::new(
                        Arc::clone(context.source_arc()),
                        target,
                        reader.cursor(),
                    );
                    let parse = self.parse_nodes(target, reader, child_context);
                    let (child_context, reader, errors) = parse.into_parts();
                    context.set_child(child_context);
                    return ParseResults::new(context, reader, errors);
                }
                potentials.push(self.parse_nodes(child_id, reader, context));
            } else {
                potentials.push(ParseResults::new(context, reader, Vec::new()));
            }
        }

        let mut potentials = potentials.into_iter();
        let Some(mut best) = potentials.next() else {
            return ParseResults::new(context_so_far, original_reader, errors);
        };
        for potential in potentials {
            if Self::is_better_parse(&potential, &best) {
                best = potential;
            }
        }
        best
    }

    fn parse_node(
        &self,
        node_id: NodeId,
        reader: &mut StringReader<'_>,
        context: &mut ParsedCommandContext<S, R>,
    ) -> Result<(), CommandSyntaxError> {
        let node = &self.nodes[node_id.index];
        let start = reader.cursor();
        match &node.data {
            CommandNodeData::Root => {
                unreachable!("the command graph never stores a root node as a child")
            }
            CommandNodeData::Literal(literal) => {
                if !reader.try_read_literal(literal) {
                    return Err(
                        reader.error(CommandSyntaxErrorKind::LiteralIncorrect(literal.to_owned()))
                    );
                }
                context.with_node(
                    node_id,
                    StringRange::between(start, reader.cursor()),
                    node.redirect.as_ref(),
                );
            }
            CommandNodeData::Argument(name, ArgumentData { argument_type, .. }) => {
                let value = argument_type.parse(reader, context.source())?;
                let range = StringRange::between(start, reader.cursor());
                context.with_argument(name, range, value);
                context.with_node(node_id, range, node.redirect.as_ref());
            }
        }
        Ok(())
    }

    fn relevant_nodes(&self, parent: NodeId, reader: &StringReader<'_>) -> Vec<NodeId> {
        let Some(parent) = self.node(parent) else {
            return Vec::new();
        };
        let remaining = reader.remaining();
        let token = remaining
            .split_once(' ')
            .map_or(remaining, |(token, _)| token);
        let mut arguments = Vec::new();

        for child_id in &parent.children {
            match &self.nodes[child_id.index].data {
                CommandNodeData::Literal(literal) if literal.as_ref() == token => {
                    return vec![*child_id];
                }
                CommandNodeData::Argument { .. } => arguments.push(*child_id),
                CommandNodeData::Root | CommandNodeData::Literal(_) => {}
            }
        }
        arguments
    }

    fn is_better_parse(
        candidate: &ParseResults<'_, S, R>,
        current: &ParseResults<'_, S, R>,
    ) -> bool {
        if !candidate.reader().can_read() && current.reader().can_read() {
            return true;
        }
        if candidate.reader().can_read() && !current.reader().can_read() {
            return false;
        }
        candidate.errors().is_empty() && !current.errors().is_empty()
    }

    pub(crate) fn has_signed_arguments(&self, parse: &ParseResults<'_, S, R>) -> bool {
        for parsed_node in parse.context().nodes() {
            if let Some(node) = self.node(parsed_node.node())
                && let CommandNodeData::Argument(_, arg_data) = &node.data
                && arg_data.argument_type().is_signed()
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn collect_signable_arguments<'a>(
        &self,
        parse: &ParseResults<'_, S, R>,
        command: &'a str,
    ) -> Vec<(String, &'a str)> {
        let mut signable_args = Vec::new();

        for parsed_node in parse.context().nodes() {
            if let Some(node) = self.node(parsed_node.node())
                && let CommandNodeData::Argument(name, arg_data) = &node.data
                && arg_data.argument_type().is_signed()
            {
                let range = parsed_node.range();

                // Map UTF-16 code unit offsets to valid UTF-8 byte bounds
                if let Some(byte_range) =
                    utf16_range_to_byte_range(command, range.start(), range.end())
                {
                    let slice = &command[byte_range];
                    signable_args.push((name.to_string(), slice));
                }
            }
        }

        signable_args
    }
}

/// Maps a UTF-16 code unit range (from Brigadier) to a byte range in a UTF-8 string.
/// Returns `None` if the indices are inverted, out of bounds, or bisect a surrogate pair.
fn utf16_range_to_byte_range(
    s: &str,
    start_utf16: usize,
    end_utf16: usize,
) -> Option<Range<usize>> {
    if start_utf16 > end_utf16 {
        return None;
    }

    let mut current_utf16 = 0;
    let mut byte_start = None;
    let mut byte_end = None;

    if start_utf16 == 0 {
        byte_start = Some(0);
    }
    if end_utf16 == 0 {
        byte_end = Some(0);
    }

    for (byte_idx, ch) in s.char_indices() {
        if current_utf16 == start_utf16 {
            byte_start = Some(byte_idx);
        }
        if current_utf16 == end_utf16 {
            byte_end = Some(byte_idx);
            break;
        }

        current_utf16 += ch.len_utf16();
    }

    // Check if bounds align with the end of the string
    if current_utf16 == start_utf16 && byte_start.is_none() {
        byte_start = Some(s.len());
    }
    if current_utf16 == end_utf16 && byte_end.is_none() {
        byte_end = Some(s.len());
    }

    match (byte_start, byte_end) {
        (Some(start), Some(end)) if start <= end && end <= s.len() => Some(start..end),
        _ => None,
    }
}

#[cfg(test)]
impl<S> CommandDispatcher<S, BrigadierRuntime> {
    pub(super) fn execute_node_for_test(
        &self,
        node: NodeId,
        source: S,
    ) -> Option<Result<i32, CommandSyntaxError>> {
        let node = self.node(node)?;
        if !node.can_execute(&source) {
            return None;
        }
        let executor = node.executor.as_deref()?;
        Some(executor(&CommandContext::empty(source, self.root())))
    }
}

impl<S, R> Default for CommandDispatcher<S, R>
where
    R: CommandRuntime<S>,
{
    fn default() -> Self {
        Self::new()
    }
}


#[cfg(test)]
mod tests {
    use super::utf16_range_to_byte_range;

    #[test]
    fn test_utf16_range_to_byte_range_ascii() {
        let text = "hello world";
        // "world" -> [6, 11]
        assert_eq!(utf16_range_to_byte_range(text, 6, 11), Some(6..11));
        assert_eq!(&text[6..11], "world");
    }

    #[test]
    fn test_utf16_range_to_byte_range_multibyte_accents() {
        // 'é' is 2 bytes in UTF-8, but 1 code unit in UTF-16
        let text = "café noir";
        // UTF-16 lengths:
        // 'c' (1), 'a' (1), 'f' (1), 'é' (1), ' ' (1), 'n' (1), 'o' (1), 'i' (1), 'r' (1)
        // "noir" starts at UTF-16 index 5 and ends at 9
        // In UTF-8 bytes:
        // 'c' (0), 'a' (1), 'f' (2), 'é' (3..5), ' ' (5), "noir" (6..10)
        let range = utf16_range_to_byte_range(text, 5, 9);
        assert_eq!(range, Some(6..10));
        assert_eq!(&text[range.expect("Invalid UTF-16 range")], "noir");
    }

    #[test]
    fn test_utf16_range_to_byte_range_surrogate_pair_emoji() {
        // '🎮' (U+1F3AE) is 4 bytes in UTF-8, but 2 code units in UTF-16 (surrogate pair)
        let text = "play 🎮 now";
        // UTF-16 offsets:
        // "play " = 5 code units
        // '🎮' = 2 code units (indices 5..7)
        // " now" = 4 code units (indices 7..11)

        // Slicing the emoji itself
        let emoji_range = utf16_range_to_byte_range(text, 5, 7);
        assert_eq!(emoji_range, Some(5..9));
        assert_eq!(&text[emoji_range.expect("Invalid UTF-16 range")], "🎮");

        // Slicing "now" after the emoji
        let now_range = utf16_range_to_byte_range(text, 8, 11);
        assert_eq!(now_range, Some(10..13));
        assert_eq!(&text[now_range.expect("Invalid UTF-16 range")], "now");
    }

    #[test]
    fn test_utf16_range_to_byte_range_invalid_bounds() {
        let text = "test";
        // Inverted range
        assert_eq!(utf16_range_to_byte_range(text, 3, 2), None);

        // Out of bounds
        assert_eq!(utf16_range_to_byte_range(text, 0, 10), None);

        // Splitting a surrogate pair should be rejected / return None
        let emoji_text = "🎮"; // 2 UTF-16 code units
        assert_eq!(utf16_range_to_byte_range(emoji_text, 0, 1), None);
    }
}
