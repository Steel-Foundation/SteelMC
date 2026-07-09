//! Branch-local command parse state.

use std::sync::Arc;

use super::{
    BrigadierRuntime, CommandRuntime, CommandSyntaxError, NodeId, StringRange, StringReader,
    argument::ParsedValue, node::CommandRedirect,
};

#[derive(Clone, Debug, PartialEq)]
struct ParsedArgument {
    range: StringRange,
    value: ParsedValue,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ParsedArguments {
    values: Vec<(Box<str>, ParsedArgument)>,
}

impl ParsedArguments {
    fn insert(&mut self, name: &str, range: StringRange, value: ParsedValue) {
        let argument = ParsedArgument { range, value };
        if let Some((_, existing)) = self
            .values
            .iter_mut()
            .find(|(existing_name, _)| existing_name.as_ref() == name)
        {
            *existing = argument;
        } else {
            self.values.push((name.into(), argument));
        }
    }

    fn boolean(&self, name: &str) -> Option<bool> {
        let Some(ParsedValue::Bool(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    fn integer(&self, name: &str) -> Option<i32> {
        let Some(ParsedValue::Integer(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    fn long(&self, name: &str) -> Option<i64> {
        let Some(ParsedValue::Long(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    fn float(&self, name: &str) -> Option<f32> {
        let Some(ParsedValue::Float(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    fn double(&self, name: &str) -> Option<f64> {
        let Some(ParsedValue::Double(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    fn string(&self, name: &str) -> Option<&str> {
        let Some(ParsedValue::String(value)) = self.argument(name) else {
            return None;
        };
        Some(value)
    }

    fn argument(&self, name: &str) -> Option<&ParsedValue> {
        self.values
            .iter()
            .find(|(argument_name, _)| argument_name.as_ref() == name)
            .map(|(_, argument)| &argument.value)
    }
}

/// A command node and the input range it consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParsedCommandNode {
    node: NodeId,
    range: StringRange,
}

impl ParsedCommandNode {
    /// Returns the parsed graph node.
    pub(crate) const fn node(self) -> NodeId {
        self.node
    }

    /// Returns the UTF-16 input range consumed by the node.
    pub(crate) const fn range(self) -> StringRange {
        self.range
    }
}

/// The successful portion of one command parse branch.
pub(crate) struct ParsedCommandContext<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    source: Arc<S>,
    root: NodeId,
    arguments: ParsedArguments,
    executor: Option<Arc<R::Executor>>,
    nodes: Vec<ParsedCommandNode>,
    range: StringRange,
    child: Option<Box<Self>>,
    modifier: Option<Arc<R::Modifier>>,
    forks: bool,
}

impl<S, R> ParsedCommandContext<S, R>
where
    R: CommandRuntime<S>,
{
    pub(super) fn new(source: Arc<S>, root: NodeId, start: usize) -> Self {
        Self {
            source,
            root,
            arguments: ParsedArguments::default(),
            executor: None,
            nodes: Vec::new(),
            range: StringRange::at(start),
            child: None,
            modifier: None,
            forks: false,
        }
    }

    pub(super) fn branch(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            root: self.root,
            arguments: self.arguments.clone(),
            executor: self.executor.as_ref().map(Arc::clone),
            nodes: self.nodes.clone(),
            range: self.range,
            child: self.child.as_ref().map(|child| Box::new(child.branch())),
            modifier: self.modifier.as_ref().map(Arc::clone),
            forks: self.forks,
        }
    }

    pub(super) fn source(&self) -> &S {
        &self.source
    }

    pub(super) const fn root(&self) -> NodeId {
        self.root
    }

    pub(super) const fn source_arc(&self) -> &Arc<S> {
        &self.source
    }

    pub(super) fn set_executor(&mut self, executor: Option<Arc<R::Executor>>) {
        self.executor = executor;
    }

    pub(super) fn with_node(
        &mut self,
        node: NodeId,
        range: StringRange,
        redirect: Option<&CommandRedirect<S, R>>,
    ) {
        self.nodes.push(ParsedCommandNode { node, range });
        self.range = StringRange::encompassing(self.range, range);
        self.modifier = redirect
            .and_then(|redirect| redirect.modifier.as_ref())
            .map(Arc::clone);
        self.forks = redirect.is_some_and(|redirect| redirect.forks);
    }

    pub(super) fn with_argument(&mut self, name: &str, range: StringRange, value: ParsedValue) {
        self.arguments.insert(name, range, value);
    }

    pub(super) fn set_child(&mut self, child: Self) {
        self.child = Some(Box::new(child));
    }

    pub(super) fn build(self, input: Arc<str>) -> Arc<CommandContext<S, R>> {
        let child = self.child.map(|child| child.build(Arc::clone(&input)));
        Arc::new(CommandContext {
            source: self.source,
            input,
            root: self.root,
            arguments: Arc::new(self.arguments),
            executor: self.executor,
            nodes: self.nodes.into(),
            range: self.range,
            child,
            modifier: self.modifier,
            forks: self.forks,
        })
    }

    pub(super) fn find_suggestion_context(&self, cursor: usize) -> Option<SuggestionContext> {
        if cursor < self.range.start() {
            return None;
        }

        if self.range.end() < cursor {
            if let Some(child) = &self.child {
                return child.find_suggestion_context(cursor);
            }
            return self.nodes.last().map_or_else(
                || {
                    Some(SuggestionContext {
                        parent: self.root,
                        start: self.range.start(),
                    })
                },
                |last| {
                    Some(SuggestionContext {
                        parent: last.node,
                        start: last.range.end() + 1,
                    })
                },
            );
        }

        let mut previous = self.root;
        for node in &self.nodes {
            if node.range.start() <= cursor && cursor <= node.range.end() {
                return Some(SuggestionContext {
                    parent: previous,
                    start: node.range.start(),
                });
            }
            previous = node.node;
        }
        Some(SuggestionContext {
            parent: previous,
            start: self.range.start(),
        })
    }

    /// Returns all nodes consumed by this parse segment.
    pub(crate) fn nodes(&self) -> &[ParsedCommandNode] {
        &self.nodes
    }

    /// Returns the range covered by this parse segment.
    pub(crate) const fn range(&self) -> StringRange {
        self.range
    }

    /// Returns whether the last parsed node has a command callback.
    pub(crate) const fn is_executable(&self) -> bool {
        self.executor.is_some()
    }

    /// Returns a parsed boolean argument.
    pub(crate) fn boolean(&self, name: &str) -> Option<bool> {
        self.arguments.boolean(name)
    }

    /// Returns a parsed integer argument.
    pub(crate) fn integer(&self, name: &str) -> Option<i32> {
        self.arguments.integer(name)
    }

    /// Returns a parsed long argument.
    pub(crate) fn long(&self, name: &str) -> Option<i64> {
        self.arguments.long(name)
    }

    /// Returns a parsed float argument.
    pub(crate) fn float(&self, name: &str) -> Option<f32> {
        self.arguments.float(name)
    }

    /// Returns a parsed double argument.
    pub(crate) fn double(&self, name: &str) -> Option<f64> {
        self.arguments.double(name)
    }

    /// Returns a parsed string argument.
    pub(crate) fn string(&self, name: &str) -> Option<&str> {
        self.arguments.string(name)
    }

    /// Returns the context reached through a redirect.
    pub(crate) fn child(&self) -> Option<&Self> {
        self.child.as_deref()
    }
}

/// Immutable parsed input supplied to commands and redirect modifiers.
pub(crate) struct CommandContext<S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    source: Arc<S>,
    input: Arc<str>,
    root: NodeId,
    arguments: Arc<ParsedArguments>,
    executor: Option<Arc<R::Executor>>,
    nodes: Arc<[ParsedCommandNode]>,
    range: StringRange,
    child: Option<Arc<Self>>,
    modifier: Option<Arc<R::Modifier>>,
    forks: bool,
}

impl<S, R> CommandContext<S, R>
where
    R: CommandRuntime<S>,
{
    /// Returns the source used for this execution stage.
    pub(crate) fn source(&self) -> &S {
        &self.source
    }

    /// Returns the complete parsed command input.
    pub(crate) fn input(&self) -> &str {
        &self.input
    }

    /// Returns the root at which this context segment began parsing.
    pub(crate) const fn root(&self) -> NodeId {
        self.root
    }

    /// Returns all nodes consumed by this context segment.
    pub(crate) fn nodes(&self) -> &[ParsedCommandNode] {
        &self.nodes
    }

    /// Returns the range covered by this context segment.
    pub(crate) const fn range(&self) -> StringRange {
        self.range
    }

    /// Returns a parsed boolean argument.
    pub(crate) fn boolean(&self, name: &str) -> Option<bool> {
        self.arguments.boolean(name)
    }

    /// Returns a parsed integer argument.
    pub(crate) fn integer(&self, name: &str) -> Option<i32> {
        self.arguments.integer(name)
    }

    /// Returns a parsed long argument.
    pub(crate) fn long(&self, name: &str) -> Option<i64> {
        self.arguments.long(name)
    }

    /// Returns a parsed float argument.
    pub(crate) fn float(&self, name: &str) -> Option<f32> {
        self.arguments.float(name)
    }

    /// Returns a parsed double argument.
    pub(crate) fn double(&self, name: &str) -> Option<f64> {
        self.arguments.double(name)
    }

    /// Returns a parsed string argument.
    pub(crate) fn string(&self, name: &str) -> Option<&str> {
        self.arguments.string(name)
    }

    /// Returns the context reached through a redirect.
    pub(crate) fn child(&self) -> Option<&Self> {
        self.child.as_deref()
    }

    pub(super) const fn child_arc(&self) -> Option<&Arc<Self>> {
        self.child.as_ref()
    }

    pub(crate) fn executor(&self) -> Option<&R::Executor> {
        self.executor.as_deref()
    }

    pub(crate) fn modifier(&self) -> Option<&R::Modifier> {
        self.modifier.as_deref()
    }

    pub(super) const fn is_forked(&self) -> bool {
        self.forks
    }

    pub(super) fn copy_for(&self, source: Arc<S>) -> Self {
        Self {
            source,
            input: Arc::clone(&self.input),
            root: self.root,
            arguments: Arc::clone(&self.arguments),
            executor: self.executor.as_ref().map(Arc::clone),
            nodes: Arc::clone(&self.nodes),
            range: self.range,
            child: self.child.as_ref().map(Arc::clone),
            modifier: self.modifier.as_ref().map(Arc::clone),
            forks: self.forks,
        }
    }

    #[cfg(test)]
    pub(super) fn empty(source: S, root: NodeId) -> Self {
        Self {
            source: Arc::new(source),
            input: Arc::from(""),
            root,
            arguments: Arc::new(ParsedArguments::default()),
            executor: None,
            nodes: Arc::from([]),
            range: StringRange::at(0),
            child: None,
            modifier: None,
            forks: false,
        }
    }
}

pub(super) struct SuggestionContext {
    pub(super) parent: NodeId,
    pub(super) start: usize,
}

/// One failed candidate node from a command parse.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParseError {
    node: NodeId,
    error: CommandSyntaxError,
}

impl ParseError {
    pub(super) const fn new(node: NodeId, error: CommandSyntaxError) -> Self {
        Self { node, error }
    }

    /// Returns the candidate node that failed.
    pub(crate) const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the candidate's syntax error.
    pub(crate) const fn error(&self) -> &CommandSyntaxError {
        &self.error
    }

    pub(super) fn into_error(self) -> CommandSyntaxError {
        self.error
    }
}

/// The best branch produced by parsing a command input.
pub(crate) struct ParseResults<'input, S, R = BrigadierRuntime>
where
    R: CommandRuntime<S>,
{
    context: ParsedCommandContext<S, R>,
    reader: StringReader<'input>,
    errors: Vec<ParseError>,
}

impl<'input, S, R> ParseResults<'input, S, R>
where
    R: CommandRuntime<S>,
{
    pub(super) const fn new(
        context: ParsedCommandContext<S, R>,
        reader: StringReader<'input>,
        errors: Vec<ParseError>,
    ) -> Self {
        Self {
            context,
            reader,
            errors,
        }
    }

    /// Returns the reader positioned where this branch stopped.
    pub(crate) const fn reader(&self) -> &StringReader<'input> {
        &self.reader
    }

    /// Returns the successfully parsed context.
    pub(crate) const fn context(&self) -> &ParsedCommandContext<S, R> {
        &self.context
    }

    /// Returns candidate errors from the stopping position.
    pub(crate) fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        ParsedCommandContext<S, R>,
        StringReader<'input>,
        Vec<ParseError>,
    ) {
        (self.context, self.reader, self.errors)
    }
}
