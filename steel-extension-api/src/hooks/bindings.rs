#[stabby::host_api]
pub mod api {
    #[cfg(feature = "host")]
    use super::super::host;
    use crate::{
        ApiResult, Hooks,
        hooks::{ActionCallback, ActionValue, FilterCallback, FilterValue, HandlerOrdering},
    };
    use stabby::{slice::Slice, str::Str};

    /// Registers an action under the actual calling extension's identity.
    pub fn register_action(
        hooks: &mut Hooks<'_>,
        callback: ActionCallback,
        orderings: Slice<'_, HandlerOrdering>,
    ) -> ApiResult<()> {
        host::register_action(hooks.host().extension.name, callback, orderings.as_slice())
    }
    /// Registers a filter under the actual calling extension's identity.
    pub fn register_filter(
        hooks: &mut Hooks<'_>,
        callback: FilterCallback,
        orderings: Slice<'_, HandlerOrdering>,
    ) -> ApiResult<()> {
        host::register_filter(hooks.host().extension.name, callback, orderings.as_slice())
    }
    /// Dispatches a shared notification after validating its payload ABI.
    pub fn do_action(tag: Str<'_>, value: ActionValue<'_>) -> ApiResult<()> {
        host::do_action(tag.as_str(), value)
    }
    /// Runs an exclusive filter chain after validating its payload ABI.
    pub fn apply_filters(tag: Str<'_>, value: FilterValue<'_>) -> ApiResult<()> {
        host::apply_filters(tag.as_str(), value)
    }
}
