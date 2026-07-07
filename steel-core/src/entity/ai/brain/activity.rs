//! Named behavior groups a brain can have

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Activity {
    Core,
    Idle,
    Fight,
    Panic,
}
