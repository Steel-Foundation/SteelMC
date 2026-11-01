//! Commands and runtime operations on temporary, domain-isolated worlds and players.
mod types;
pub use types::*;
mod bindings;
pub use bindings::api;

// Server handles wrap a shared borrow, so copies alias read-only access like `&T` does.
impl Copy for api::Server<'_> {}
#[expect(
    clippy::expl_impl_clone_on_copy,
    reason = "opaque storage is private, so Clone must copy the handle without accessing its fields"
)]
impl Clone for api::Server<'_> {
    fn clone(&self) -> Self {
        *self
    }
}
