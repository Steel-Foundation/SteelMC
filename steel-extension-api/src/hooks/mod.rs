//! Typed actions and sequential filters with startup-only handler registration.

mod bindings;
#[cfg(feature = "host")]
pub(crate) mod host;
#[cfg(feature = "host")]
mod ordering;
mod value;

pub(crate) use bindings::api;
pub use value::{ActionCallback, ActionValue, FilterCallback, FilterValue};

use crate::{ApiResult, Hooks};
use stabby::{IStable, slice::Slice, str::Str, string::String as ApiString};

/// A shared notification. Tags belong to the extension defining the payload.
/// Changing a payload's meaning requires a new tag or Stabby invariant version.
pub trait Action: IStable + 'static {
    /// Versioned, owner-scoped event identity, such as `example:round_started/v1`.
    const TAG: &'static str;
}

/// A value transformed in place by handlers, in their resolved order.
pub trait Filter: IStable + 'static {
    /// Versioned, owner-scoped filter identity.
    const TAG: &'static str;
}

/// Direction of an ordering constraint relative to another extension.
#[stabby::stabby]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderingConstraint {
    /// Run before all handlers belonging to the named extension.
    Before,
    /// Run after all handlers belonging to the named extension.
    After,
}

/// One handler ordering constraint. An absent extension imposes no constraint.
#[stabby::stabby]
#[derive(Clone, Debug)]
pub struct HandlerOrdering {
    /// Requested direction.
    pub constraint: OrderingConstraint,
    /// Descriptor name of the other extension.
    pub extension: ApiString,
}

impl<'hooks> Hooks<'hooks> {
    /// Prepares a shared action handler for registration during pre-init.
    pub fn add_action<A: Action>(
        &mut self,
        callback: extern "C" fn(&A),
    ) -> ActionBuilder<'_, 'hooks> {
        ActionBuilder {
            hooks: self,
            callback: ActionCallback::new(callback),
            orderings: Vec::new(),
        }
    }

    /// Prepares a filter handler for registration during pre-init.
    pub fn add_filter<F: Filter>(
        &mut self,
        callback: extern "C" fn(&mut F),
    ) -> FilterBuilder<'_, 'hooks> {
        FilterBuilder {
            hooks: self,
            callback: FilterCallback::new(callback),
            orderings: Vec::new(),
        }
    }
}

/// An action registration with optional ordering constraints.
pub struct ActionBuilder<'borrow, 'hooks> {
    hooks: &'borrow mut Hooks<'hooks>,
    callback: ActionCallback,
    orderings: Vec<HandlerOrdering>,
}

/// A filter registration with optional ordering constraints.
pub struct FilterBuilder<'borrow, 'hooks> {
    hooks: &'borrow mut Hooks<'hooks>,
    callback: FilterCallback,
    orderings: Vec<HandlerOrdering>,
}

macro_rules! ordering_builder {
    ($builder:ident, $register:ident) => {
        impl $builder<'_, '_> {
            /// Runs before handlers belonging to `extension` for this event.
            #[must_use]
            pub fn before(mut self, extension: &str) -> Self {
                self.orderings.push(HandlerOrdering {
                    constraint: OrderingConstraint::Before,
                    extension: extension.into(),
                });
                self
            }
            /// Runs after handlers belonging to `extension` for this event.
            #[must_use]
            pub fn after(mut self, extension: &str) -> Self {
                self.orderings.push(HandlerOrdering {
                    constraint: OrderingConstraint::After,
                    extension: extension.into(),
                });
                self
            }
            /// Installs this handler. Ordering cycles are rejected before extension init.
            pub fn register(self) -> ApiResult<()> {
                api::$register(
                    self.hooks,
                    self.callback,
                    Slice::from(self.orderings.as_slice()),
                )
            }
        }
    };
}
ordering_builder!(ActionBuilder, register_action);
ordering_builder!(FilterBuilder, register_filter);

/// Dispatches an action synchronously after startup registration has frozen.
/// Nested dispatch is supported; the host holds no registration lock during callbacks.
pub fn do_action<A: Action>(value: &A) -> ApiResult<()> {
    api::do_action(Str::new(A::TAG), ActionValue::new(value))
}

/// Applies filters synchronously in order. A rejected payload is never passed to a handler.
pub fn apply_filters<F: Filter>(value: &mut F) -> ApiResult<()> {
    api::apply_filters(Str::new(F::TAG), FilterValue::new(value))
}

/// Notification emitted before worlds run on each unfrozen server tick.
#[stabby::stabby]
pub struct ServerTickAction {
    /// Current server tick number.
    pub tick_count: u64,
}
impl Action for ServerTickAction {
    const TAG: &'static str = "steel:server_tick/v1";
}

/// Notification emitted once before shutdown saves begin.
#[stabby::stabby]
pub struct ServerStopAction;
impl Action for ServerStopAction {
    const TAG: &'static str = "steel:server_stop/v1";
}
