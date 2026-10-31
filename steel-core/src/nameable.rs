//! Names exposed by gameplay objects.

use text_components::TextComponent;

/// An object's effective name and optional custom name.
pub trait Nameable: Send + Sync {
    /// Custom name, without substituting the object's default name.
    fn custom_name(&self) -> Option<TextComponent>;

    /// Custom name when present, otherwise the object's default name.
    fn name(&self) -> TextComponent;
}
