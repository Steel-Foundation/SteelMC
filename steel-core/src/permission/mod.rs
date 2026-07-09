//! Steel's internal permission evaluation model.

mod context;
mod expression;
mod groups;
mod key;
mod rule_expression;
mod set;

pub use context::{
    PermissionContext, PermissionContextKey, PermissionContextKeyError, PermissionRuleContext,
    PermissionRuleContextError, PermissionRuleContexts,
};
pub use expression::PermissionExpr;
pub use groups::{
    PermissionConfigError, PermissionGroup, PermissionGroupConfig, PermissionGroups,
    PermissionGroupsConfig,
};
pub use key::{PermissionKey, PermissionKeyError, PermissionSegment};
pub use rule_expression::{PermissionRuleExpression, PermissionRuleExpressionError};
pub use set::{
    PermissionEntry, PermissionResolution, PermissionResolutionSource, PermissionSet,
    PermissionState,
};

#[cfg(test)]
mod group_tests;
#[cfg(test)]
mod tests;
