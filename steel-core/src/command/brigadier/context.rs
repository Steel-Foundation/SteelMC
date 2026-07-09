//! Branch-local command parse state.

use std::sync::Arc;

use super::{
    CommandSyntaxError, NodeId, StringRange, StringReader, argument::ParsedValue, node::Command,
};

#[derive(Clone, Debug, PartialEq)]
struct ParsedArgument {
    range: StringRange,
    value: ParsedValue,
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
pub(crate) struct ParsedCommandContext<S> {
    source: Arc<S>,
    root: NodeId,
    arguments: Vec<(Box<str>, ParsedArgument)>,
    command: Option<Command<S>>,
    nodes: Vec<ParsedCommandNode>,
    range: StringRange,
    child: Option<Box<Self>>,
}

impl<S> ParsedCommandContext<S> {
    pub(super) fn new(source: Arc<S>, root: NodeId, start: usize) -> Self {
        Self {
            source,
            root,
            arguments: Vec::new(),
            command: None,
            nodes: Vec::new(),
            range: StringRange::at(start),
            child: None,
        }
    }

    pub(super) fn branch(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            root: self.root,
            arguments: self
                .arguments
                .iter()
                .map(|(name, argument)| (name.to_owned(), argument.clone()))
                .collect(),
            command: self.command.as_ref().map(Arc::clone),
            nodes: self.nodes.clone(),
            range: self.range,
            child: self.child.as_ref().map(|child| Box::new(child.branch())),
        }
    }

    pub(super) fn source(&self) -> &S {
        &self.source
    }

    pub(super) const fn source_arc(&self) -> &Arc<S> {
        &self.source
    }

    pub(super) fn set_command(&mut self, command: Option<Command<S>>) {
        self.command = command;
    }

    pub(super) fn with_node(&mut self, node: NodeId, range: StringRange) {
        self.nodes.push(ParsedCommandNode { node, range });
        self.range = StringRange::encompassing(self.range, range);
    }

    pub(super) fn with_argument(&mut self, name: &str, range: StringRange, value: ParsedValue) {
        let argument = ParsedArgument { range, value };
        if let Some((_, existing)) = self
            .arguments
            .iter_mut()
            .find(|(existing_name, _)| existing_name.as_ref() == name)
        {
            *existing = argument;
        } else {
            self.arguments.push((name.into(), argument));
        }
    }

    pub(super) fn set_child(&mut self, child: Self) {
        self.child = Some(Box::new(child));
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
        self.command.is_some()
    }

    /// Returns a parsed boolean argument.
    pub(crate) fn boolean(&self, name: &str) -> Option<bool> {
        let Some(ParsedValue::Bool(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    /// Returns a parsed integer argument.
    pub(crate) fn integer(&self, name: &str) -> Option<i32> {
        let Some(ParsedValue::Integer(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    /// Returns a parsed long argument.
    pub(crate) fn long(&self, name: &str) -> Option<i64> {
        let Some(ParsedValue::Long(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    /// Returns a parsed float argument.
    pub(crate) fn float(&self, name: &str) -> Option<f32> {
        let Some(ParsedValue::Float(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    /// Returns a parsed double argument.
    pub(crate) fn double(&self, name: &str) -> Option<f64> {
        let Some(ParsedValue::Double(value)) = self.argument(name) else {
            return None;
        };
        Some(*value)
    }

    /// Returns a parsed string argument.
    pub(crate) fn string(&self, name: &str) -> Option<&str> {
        let Some(ParsedValue::String(value)) = self.argument(name) else {
            return None;
        };
        Some(value)
    }

    /// Returns the context reached through a redirect.
    pub(crate) fn child(&self) -> Option<&Self> {
        self.child.as_deref()
    }

    fn argument(&self, name: &str) -> Option<&ParsedValue> {
        self.arguments
            .iter()
            .find(|(argument_name, _)| argument_name.as_ref() == name)
            .map(|(_, argument)| &argument.value)
    }
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
}

/// The best branch produced by parsing a command input.
pub(crate) struct ParseResults<'input, S> {
    context: ParsedCommandContext<S>,
    reader: StringReader<'input>,
    errors: Vec<ParseError>,
}

impl<'input, S> ParseResults<'input, S> {
    pub(super) const fn new(
        context: ParsedCommandContext<S>,
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
    pub(crate) const fn context(&self) -> &ParsedCommandContext<S> {
        &self.context
    }

    /// Returns candidate errors from the stopping position.
    pub(crate) fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        ParsedCommandContext<S>,
        StringReader<'input>,
        Vec<ParseError>,
    ) {
        (self.context, self.reader, self.errors)
    }
}
