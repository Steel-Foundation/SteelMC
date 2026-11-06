//! Per-type properties: the values and hooks vanilla subclasses override
//! without calling `super`.
//!
//! [`entity_props!`] declares a layer's hooks once and generates:
//!
//! - `XProps<E>`: one `fn(&E, ..)` per hook, typed for the concrete entity.
//!   `XProps::new()` holds the layer's defaults; presets and entity types
//!   override fields with struct-update syntax.
//! - `XHooks`: the same hooks reached through the layer's view, implemented
//!   once for every `XProps<E>`. It downcasts the view's entity to `E` (with
//!   the keyed downcast) and falls back to the default on a mismatch, which
//!   only a wiring bug can cause.
//! - one method per hook on the view, so callers never see the plumbing.
//!
//! Each hook names the view method holding its default body.

/// Declares a layer's per-type hooks; see the [module docs](self).
macro_rules! entity_props {
    (
        $(#[$meta:meta])*
        $vis:vis struct $props:ident<E>: $hooks:ident for $view:ident
        where E: $parts:path => $view_fn:ident {
            $(
                $(#[$hook_meta:meta])*
                fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? = $default:ident;
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
            /// The layer's defaults.
            #[must_use]
            pub const fn new() -> Self {
                Self {
                    $( $name: |entity $(, $arg)*| entity.$view_fn().$default($($arg),*), )*
                }
            }
        }

        impl<E: $parts> Default for $props<E> {
            fn default() -> Self {
                Self::new()
            }
        }

        #[doc = concat!("[`", stringify!($props), "`] reached through [`", stringify!($view), "`].")]
        $vis trait $hooks: Send + Sync {
            $(
                $(#[$hook_meta])*
                fn $name(&self, view: &$view<'_> $(, $arg: $ty)*) $(-> $ret)?;
            )*
        }

        impl<E: $parts + ::steel_utils::DowncastType> $hooks for $props<E> {
            $(
                fn $name(&self, view: &$view<'_> $(, $arg: $ty)*) $(-> $ret)? {
                    match <dyn $crate::entity::Entity as ::steel_utils::Downcast>::downcast_ref::<E>(
                        view.entity(),
                    ) {
                        Some(entity) => (self.$name)(entity $(, $arg)*),
                        None => view.$default($($arg),*),
                    }
                }
            )*
        }

        impl $view<'_> {
            $(
                $crate::entity::component::props::entity_props!(
                    @dispatch $(#[$hook_meta])* $name($($arg: $ty),*) $(-> $ret)?
                );
            )*
        }
    };
    (@dispatch $(#[$hook_meta:meta])* $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        $(#[$hook_meta])*
        #[must_use]
        pub fn $name(&self $(, $arg: $ty)*) -> $ret {
            self.hooks().$name(self $(, $arg)*)
        }
    };
    (@dispatch $(#[$hook_meta:meta])* $name:ident($($arg:ident: $ty:ty),*)) => {
        $(#[$hook_meta])*
        pub fn $name(&self $(, $arg: $ty)*) {
            self.hooks().$name(self $(, $arg)*);
        }
    };
}

pub(crate) use entity_props;
