//! Module defining errors that can occur during command execution.
use text_components::TextComponent;

/// An error that can occur during command execution.
pub enum CommandError {
    /// This error means that there was an error while parsing a previously consumed argument.
    /// That only happens when consumption is wrongly implemented, as it should ensure parsing may
    /// never fail.
    InvalidConsumption(Option<String>),
    /// Return this if a condition that a [`Node::Require`] should ensure is met is not met.
    InvalidRequirement,
    /// The command could not be executed due to insufficient permissions.
    /// The user attempting to run the command lacks the necessary authorization.
    PermissionDenied,
    /// A general error occurred during command execution that doesn't fit into
    /// more specific `CommandError` variants.
    CommandFailed(Box<TextComponent>),
}
