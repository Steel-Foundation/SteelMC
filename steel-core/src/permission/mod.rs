//! Steel's internal permission evaluation model.

mod context;
mod expression;
mod key;
mod set;

pub use context::{
    PermissionContext, PermissionContextKey, PermissionContextKeyError, PermissionRuleContext,
    PermissionRuleContextError, PermissionRuleContexts,
};
pub use expression::PermissionExpr;
pub use key::{PermissionKey, PermissionKeyError, PermissionSegment};
pub use set::{
    PermissionEntry, PermissionResolution, PermissionResolutionSource, PermissionSet,
    PermissionState,
};

#[cfg(test)]
mod tests;
