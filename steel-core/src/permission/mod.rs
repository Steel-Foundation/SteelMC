//! Steel's internal permission evaluation model.

mod context;
mod expression;
mod groups;
mod key;
mod manager;
mod metadata;
mod rule_expression;
mod set;
mod subject;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/groups.rs"]
mod generated_groups;

pub use generated_groups::{
    ADMINS_GROUP, ALL_GROUP, COMMAND_BLOCK_GROUPS, DEFAULT_GROUP, GAMEMASTERS_GROUP, GROUPS,
    MODERATORS_GROUP, OP_GROUP, OWNERS_GROUP,
};

pub use context::{
    PermissionContext, PermissionContextKey, PermissionContextKeyError, PermissionContextValue,
    PermissionDomain, PermissionRuleContext, PermissionRuleContextError, PermissionRuleContexts,
};
pub use expression::PermissionExpr;
pub use groups::{
    PermissionConfigError, PermissionGroup, PermissionGroupConfig, PermissionGroups,
    PermissionGroupsConfig, PermissionMetadataRuleConfig,
};
pub use key::{PermissionKey, PermissionKeyError, PermissionSegment};
pub use manager::{
    PermissionGroupManager, PermissionGroupManagerError, PermissionGroupStore,
    PermissionGroupStoreError, PermissionGroupUpdateError,
};
pub use metadata::{
    PermissionMetadataEntry, PermissionMetadataExpression, PermissionMetadataExpressionError,
    PermissionMetadataKeyError, PermissionMetadataResolution, PermissionMetadataSet,
    PermissionMetadataValue, parse_permission_metadata_key,
};
pub use rule_expression::{PermissionRuleExpression, PermissionRuleExpressionError};
pub use set::{
    PermissionEntry, PermissionResolution, PermissionResolutionSource, PermissionSet,
    PermissionState,
};
pub use subject::{PermissionSubjectIndex, PermissionSubjectState};

#[cfg(test)]
mod tests;
