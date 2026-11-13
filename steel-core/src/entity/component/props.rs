//! Per-type properties: the methods of one vanilla class that its subclasses
//! override.
//!
//! [`entity_props!`] declares a layer's hooks once and generates:
//!
//! - `XProps<E>`: one `fn(&E, ..)` per hook, typed for the concrete entity.
//!   `XProps::new()` holds the declaring class's own bodies; presets and
//!   entity types override fields with struct-update syntax.
//! - `XHooks`: the same hooks as methods of the entity. It is implemented
//!   once, for every type with the layer's table trait, by reading the type's
//!   `const` table: a call lands on an entry known at compile time, and a
//!   table of another type does not compile. It also hands out the layer's
//!   component, so a view can be built from `dyn Entity` without a keyed
//!   lookup.
//! - one method per hook on the view, so callers never see the plumbing.
//!
//! The declaring class's body for a hook is the function of the same name in
//! the module that invokes the macro (`living_entity::ai_step`); how overrides
//! call `super` is described in [`crate::entity::component`]. The layer
//! declares the table trait itself (`trait XTable { const X: XProps<Self>; }`)
//! and stores the `&dyn XHooks` its view dispatches through.

/// Declares a layer's per-type hooks; see the [module docs](self).
macro_rules! entity_props {
    (
        $(#[$meta:meta])*
        $vis:vis struct $props:ident<E>: $hooks:ident for $view:ident
        where E: $parts:path,
        table $table:ident::$table_const:ident,
        component $component:ty {
            $(
                $(#[$hook_meta:meta])*
                fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;
            )*
        }
    ) => {
        $(#[$meta])*
        $vis struct $props<E: 'static> {
            $(
                $(#[$hook_meta])*
                pub $name: fn(&E $(, $ty)*) $(-> $ret)?,
            )*
        }

        impl<E: $parts> $props<E> {
            /// The declaring class's own bodies.
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    $( $name: $name::<E>, )*
                }
            }
        }

        impl<E: $parts> Default for $props<E> {
            fn default() -> Self {
                Self::new()
            }
        }

        #[doc = concat!(
            "[`", stringify!($props), "`] as methods of the entity. Implemented for every [`",
            stringify!($table), "`]; [`", stringify!($view), "`] dispatches through it."
        )]
        $vis trait $hooks: Send + Sync {
            /// The layer's component of this entity.
            fn layer_component(&self) -> &$component;

            $(
                $(#[$hook_meta])*
                fn $name(&self $(, $arg: $ty)*) $(-> $ret)?;
            )*
        }

        impl<T: $table> $hooks for T {
            fn layer_component(&self) -> &$component {
                $crate::entity::component::Has::<$component>::component(self)
            }

            $(
                fn $name(&self $(, $arg: $ty)*) $(-> $ret)? {
                    (T::$table_const.$name)(self $(, $arg)*)
                }
            )*
        }

        impl $view<'_> {
            $(
                $crate::entity::component::props::entity_props!(
                    @dispatch $hooks $(#[$hook_meta])* $name($($arg: $ty),*) $(-> $ret)?
                );
            )*
        }
    };
    (@dispatch $hooks:ident $(#[$hook_meta:meta])* $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        $(#[$hook_meta])*
        #[must_use]
        pub fn $name(&self $(, $arg: $ty)*) -> $ret {
            <dyn $hooks>::$name(self.hooks $(, $arg)*)
        }
    };
    (@dispatch $hooks:ident $(#[$hook_meta:meta])* $name:ident($($arg:ident: $ty:ty),*)) => {
        $(#[$hook_meta])*
        pub fn $name(&self $(, $arg: $ty)*) {
            <dyn $hooks>::$name(self.hooks $(, $arg)*);
        }
    };
}

pub(crate) use entity_props;
