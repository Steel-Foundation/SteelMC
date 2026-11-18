//! Steel's internal permission evaluation model.

mod collection;
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
#[path = "generated/collections.rs"]
mod generated_collections;

pub use collection::{
    PermissionCollection, PermissionCollectionError, PermissionCollectionMember,
    PermissionCollections, PermissionCollectionsConfig,
};
pub use generated_collections::{
    ADMINS_COLLECTION, ADMINS_GROUP, ALL_COLLECTION, ALL_GROUP, COMMAND_BLOCK_GROUPS,
    DEFAULT_GROUP, GAMEMASTERS_COLLECTION, GAMEMASTERS_GROUP, GROUPS, MODERATORS_COLLECTION,
    MODERATORS_GROUP, OP_GROUP, OWNERS_COLLECTION, OWNERS_GROUP,
};

pub use context::{
    PermissionContext, PermissionContextKey, PermissionContextKeyError, PermissionContextValue,
    PermissionDomain, PermissionRuleContext, PermissionRuleContextError, PermissionRuleContexts,
};
pub use expression::PermissionExpr;
pub use groups::{
    PermissionConfigError, PermissionGroup, PermissionGroupConfig, PermissionGroupRule,
    PermissionGroupRuleError, PermissionGroups, PermissionGroupsConfig,
    PermissionMetadataRuleConfig,
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
