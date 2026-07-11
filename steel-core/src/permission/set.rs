use std::mem;

use super::{PermissionContext, PermissionExpr, PermissionKey, PermissionRuleContext};

/// Resolved state of one matching permission rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    /// Explicitly grants the matching permission.
    Allow,
    /// Explicitly rejects the matching permission.
    Deny,
}

/// One permission rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionEntry {
    key: PermissionKey,
    context: PermissionRuleContext,
    state: PermissionState,
}

impl PermissionEntry {
    /// Creates one global permission rule.
    #[must_use]
    pub const fn new(key: PermissionKey, state: PermissionState) -> Self {
        Self {
            key,
            context: PermissionRuleContext::Global,
            state,
        }
    }

    /// Creates one contextual permission rule.
    #[must_use]
    pub const fn new_with_context(
        key: PermissionKey,
        context: PermissionRuleContext,
        state: PermissionState,
    ) -> Self {
        Self {
            key,
            context,
            state,
        }
    }

    /// Creates a global allow rule.
    #[must_use]
    pub const fn allow(key: PermissionKey) -> Self {
        Self::new(key, PermissionState::Allow)
    }

    /// Creates a contextual allow rule.
    #[must_use]
    pub const fn allow_with_context(key: PermissionKey, context: PermissionRuleContext) -> Self {
        Self::new_with_context(key, context, PermissionState::Allow)
    }

    /// Creates a global deny rule.
    #[must_use]
    pub const fn deny(key: PermissionKey) -> Self {
        Self::new(key, PermissionState::Deny)
    }

    /// Creates a contextual deny rule.
    #[must_use]
    pub const fn deny_with_context(key: PermissionKey, context: PermissionRuleContext) -> Self {
        Self::new_with_context(key, context, PermissionState::Deny)
    }

    /// Returns the key pattern matched by this rule.
    #[must_use]
    pub const fn key(&self) -> &PermissionKey {
        &self.key
    }

    /// Returns the runtime context constraint.
    #[must_use]
    pub const fn context(&self) -> &PermissionRuleContext {
        &self.context
    }

    /// Returns whether this rule allows or denies.
    #[must_use]
    pub const fn state(&self) -> PermissionState {
        self.state
    }
}

/// A flat effective permission set with deterministic conflict resolution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionSet {
    entries: Vec<PermissionEntry>,
    sources: Vec<PermissionResolutionSource>,
}

impl PermissionSet {
    /// Creates an empty set. Unset permissions resolve to `None` and are denied by `allows*`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            sources: Vec::new(),
        }
    }

    /// Creates a set of direct subject rules.
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = PermissionEntry>) -> Self {
        let entries = entries.into_iter().collect::<Vec<_>>();
        let sources = vec![PermissionResolutionSource::Subject; entries.len()];
        Self { entries, sources }
    }

    /// Returns all rules in insertion order.
    #[must_use]
    pub fn entries(&self) -> &[PermissionEntry] {
        &self.entries
    }

    /// Adds a direct subject rule.
    pub fn push(&mut self, entry: PermissionEntry) {
        self.push_with_source(entry, PermissionResolutionSource::Subject);
    }

    /// Adds a global direct allow.
    pub fn allow(&mut self, key: PermissionKey) {
        self.push(PermissionEntry::allow(key));
    }

    /// Adds a contextual direct allow.
    pub fn allow_in(&mut self, key: PermissionKey, context: PermissionRuleContext) {
        self.push(PermissionEntry::allow_with_context(key, context));
    }

    /// Adds a global direct deny.
    pub fn deny(&mut self, key: PermissionKey) {
        self.push(PermissionEntry::deny(key));
    }

    /// Adds a contextual direct deny.
    pub fn deny_in(&mut self, key: PermissionKey, context: PermissionRuleContext) {
        self.push(PermissionEntry::deny_with_context(key, context));
    }

    /// Replaces the exact global rule for `key`.
    pub fn set(&mut self, key: PermissionKey, state: PermissionState) {
        self.set_in(key, PermissionRuleContext::Global, state);
    }

    /// Replaces the exact rule for `key` and `context`.
    pub fn set_in(
        &mut self,
        key: PermissionKey,
        context: PermissionRuleContext,
        state: PermissionState,
    ) {
        self.retain_entries(|entry| entry.key != key || entry.context != context);
        self.push(PermissionEntry::new_with_context(key, context, state));
    }

    /// Removes the exact global rule for `key`.
    pub fn unset(&mut self, key: &PermissionKey) -> bool {
        self.unset_in(key, &PermissionRuleContext::Global)
    }

    /// Removes the exact rule for `key` and `context`.
    pub fn unset_in(&mut self, key: &PermissionKey, context: &PermissionRuleContext) -> bool {
        let old_len = self.entries.len();
        self.retain_entries(|entry| entry.key() != key || entry.context() != context);
        self.entries.len() != old_len
    }

    /// Resolves one key globally.
    #[must_use]
    pub fn resolve_key(&self, key: &PermissionKey) -> Option<PermissionState> {
        self.resolve_key_in(key, &PermissionContext::global())
    }

    /// Resolves one key in an active context.
    #[must_use]
    pub fn resolve_key_in(
        &self,
        key: &PermissionKey,
        context: &PermissionContext,
    ) -> Option<PermissionState> {
        self.best_key_candidate(key, context)
            .map(|candidate| candidate.state)
    }

    /// Resolves one key globally and returns the winning rule details.
    #[must_use]
    pub fn resolve_key_detailed(&self, key: &PermissionKey) -> Option<PermissionResolution> {
        self.resolve_key_in_detailed(key, &PermissionContext::global())
    }

    /// Resolves one key in an active context and returns the winning rule details.
    #[must_use]
    pub fn resolve_key_in_detailed(
        &self,
        key: &PermissionKey,
        context: &PermissionContext,
    ) -> Option<PermissionResolution> {
        self.best_key_candidate(key, context)
            .map(|candidate| self.permission_resolution(candidate))
    }

    /// Resolves a child key that may inherit a broad parent grant.
    #[must_use]
    pub fn resolve_scoped_key(
        &self,
        parent: &PermissionKey,
        key: &PermissionKey,
    ) -> Option<PermissionState> {
        self.resolve_scoped_key_in(parent, key, &PermissionContext::global())
    }

    /// Resolves a child key with a parent fallback in an active context.
    #[must_use]
    pub fn resolve_scoped_key_in(
        &self,
        parent: &PermissionKey,
        key: &PermissionKey,
        context: &PermissionContext,
    ) -> Option<PermissionState> {
        self.best_scoped_key_candidate(parent, key, context)
            .map(|candidate| candidate.state)
    }

    /// Resolves a scoped child globally and returns the winning rule details.
    #[must_use]
    pub fn resolve_scoped_key_detailed(
        &self,
        parent: &PermissionKey,
        key: &PermissionKey,
    ) -> Option<PermissionResolution> {
        self.resolve_scoped_key_in_detailed(parent, key, &PermissionContext::global())
    }

    /// Resolves a scoped child in an active context and returns winning rule details.
    #[must_use]
    pub fn resolve_scoped_key_in_detailed(
        &self,
        parent: &PermissionKey,
        key: &PermissionKey,
        context: &PermissionContext,
    ) -> Option<PermissionResolution> {
        self.best_scoped_key_candidate(parent, key, context)
            .map(|candidate| self.permission_resolution(candidate))
    }

    /// Returns whether one global key resolves to allow.
    #[must_use]
    pub fn allows_key(&self, key: &PermissionKey) -> bool {
        self.resolve_key(key) == Some(PermissionState::Allow)
    }

    /// Returns whether one key resolves to allow in `context`.
    #[must_use]
    pub fn allows_key_in(&self, key: &PermissionKey, context: &PermissionContext) -> bool {
        self.resolve_key_in(key, context) == Some(PermissionState::Allow)
    }

    /// Returns whether one scoped child resolves to allow globally.
    #[must_use]
    pub fn allows_scoped_key(&self, parent: &PermissionKey, key: &PermissionKey) -> bool {
        self.resolve_scoped_key(parent, key) == Some(PermissionState::Allow)
    }

    /// Returns whether one scoped child resolves to allow in `context`.
    #[must_use]
    pub fn allows_scoped_key_in(
        &self,
        parent: &PermissionKey,
        key: &PermissionKey,
        context: &PermissionContext,
    ) -> bool {
        self.resolve_scoped_key_in(parent, key, context) == Some(PermissionState::Allow)
    }

    /// Returns whether a global permission expression resolves to allow.
    #[must_use]
    pub fn allows(&self, permission: &PermissionExpr) -> bool {
        self.allows_in(permission, &PermissionContext::global())
    }

    /// Returns whether an expression resolves to allow in `context`.
    #[must_use]
    pub fn allows_in(&self, permission: &PermissionExpr, context: &PermissionContext) -> bool {
        self.resolve_in(permission, context) == Some(PermissionState::Allow)
    }

    /// Resolves a permission expression globally.
    #[must_use]
    pub fn resolve(&self, permission: &PermissionExpr) -> Option<PermissionState> {
        self.resolve_in(permission, &PermissionContext::global())
    }

    /// Resolves a permission expression in `context`.
    #[must_use]
    pub fn resolve_in(
        &self,
        permission: &PermissionExpr,
        context: &PermissionContext,
    ) -> Option<PermissionState> {
        match permission {
            PermissionExpr::Key(key) => self.resolve_key_in(key, context),
            PermissionExpr::ScopedKey { parent, key } => {
                self.resolve_scoped_key_in(parent, key, context)
            }
            PermissionExpr::All(children) => resolve_all(children, self, context),
            PermissionExpr::Any(children) => resolve_any(children, self, context),
        }
    }

    pub(super) fn push_group(&mut self, entry: PermissionEntry, group: &str, group_priority: i32) {
        self.push_with_source(
            entry,
            PermissionResolutionSource::Group {
                name: group.to_owned(),
                priority: group_priority,
            },
        );
    }

    fn push_with_source(&mut self, entry: PermissionEntry, source: PermissionResolutionSource) {
        self.entries.push(entry);
        self.sources.push(source);
    }

    fn retain_entries(&mut self, mut keep: impl FnMut(&PermissionEntry) -> bool) {
        let entries = mem::take(&mut self.entries);
        let sources = mem::take(&mut self.sources);
        for (entry, source) in entries.into_iter().zip(sources) {
            if keep(&entry) {
                self.entries.push(entry);
                self.sources.push(source);
            }
        }
    }

    fn best_key_candidate(
        &self,
        key: &PermissionKey,
        context: &PermissionContext,
    ) -> Option<PermissionCandidate> {
        let mut best = None;
        for (index, (entry, source)) in self.entries.iter().zip(&self.sources).enumerate() {
            if !entry.context.matches(context) || !entry.key.matches(key) {
                continue;
            }
            push_candidate(
                &mut best,
                index,
                entry.key.specificity(),
                entry.context.specificity(),
                source,
                entry.state,
            );
        }
        best
    }

    fn best_scoped_key_candidate(
        &self,
        parent: &PermissionKey,
        key: &PermissionKey,
        context: &PermissionContext,
    ) -> Option<PermissionCandidate> {
        let mut best = None;
        let parent_scopes_key = parent.scopes(key);
        for (index, (entry, source)) in self.entries.iter().zip(&self.sources).enumerate() {
            if !entry.context.matches(context) {
                continue;
            }
            let matches_parent = parent_scopes_key && entry.key.matches(parent);
            let matches_key = entry.key.matches(key);
            if !matches_parent && !matches_key {
                continue;
            }

            let mut specificity = entry.key.specificity();
            if matches_key && !matches_parent {
                specificity += 1;
            }
            push_candidate(
                &mut best,
                index,
                specificity,
                entry.context.specificity(),
                source,
                entry.state,
            );
        }
        best
    }

    fn permission_resolution(&self, candidate: PermissionCandidate) -> PermissionResolution {
        PermissionResolution {
            entry: self.entries[candidate.entry_index].clone(),
            source: candidate.source,
            key_specificity: candidate.key_specificity,
            context_specificity: candidate.context_specificity,
        }
    }
}

fn resolve_all(
    children: &[PermissionExpr],
    permissions: &PermissionSet,
    context: &PermissionContext,
) -> Option<PermissionState> {
    if children.is_empty() {
        return Some(PermissionState::Allow);
    }
    let mut saw_unset = false;
    for child in children {
        match permissions.resolve_in(child, context) {
            Some(PermissionState::Allow) => {}
            Some(PermissionState::Deny) => return Some(PermissionState::Deny),
            None => saw_unset = true,
        }
    }
    if saw_unset {
        None
    } else {
        Some(PermissionState::Allow)
    }
}

fn resolve_any(
    children: &[PermissionExpr],
    permissions: &PermissionSet,
    context: &PermissionContext,
) -> Option<PermissionState> {
    if children.is_empty() {
        return Some(PermissionState::Deny);
    }
    let mut saw_unset = false;
    for child in children {
        match permissions.resolve_in(child, context) {
            Some(PermissionState::Allow) => return Some(PermissionState::Allow),
            Some(PermissionState::Deny) => {}
            None => saw_unset = true,
        }
    }
    if saw_unset {
        None
    } else {
        Some(PermissionState::Deny)
    }
}

/// Origin of a rule in an effective permission set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionResolutionSource {
    /// A configured permission group contributed the rule.
    Group {
        /// Group name.
        name: String,
        /// Priority used for equally specific group conflicts.
        priority: i32,
    },
    /// A direct subject override contributed the rule.
    Subject,
}

impl PermissionResolutionSource {
    /// Returns the contributing group name.
    #[must_use]
    pub fn group_name(&self) -> Option<&str> {
        match self {
            Self::Group { name, .. } => Some(name),
            Self::Subject => None,
        }
    }

    /// Returns the contributing group priority.
    #[must_use]
    pub const fn group_priority(&self) -> Option<i32> {
        match self {
            Self::Group { priority, .. } => Some(*priority),
            Self::Subject => None,
        }
    }

    pub(super) const fn rank(&self) -> usize {
        match self {
            Self::Group { .. } => 0,
            Self::Subject => 1,
        }
    }

    pub(super) const fn tie_priority(&self) -> i32 {
        match self {
            Self::Group { priority, .. } => *priority,
            Self::Subject => 0,
        }
    }
}

/// Detailed winning permission rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionResolution {
    entry: PermissionEntry,
    source: PermissionResolutionSource,
    key_specificity: usize,
    context_specificity: usize,
}

impl PermissionResolution {
    /// Returns the complete winning rule.
    #[must_use]
    pub const fn entry(&self) -> &PermissionEntry {
        &self.entry
    }

    /// Returns where the winning rule came from.
    #[must_use]
    pub const fn source(&self) -> &PermissionResolutionSource {
        &self.source
    }

    /// Returns the winning allow or deny state.
    #[must_use]
    pub const fn state(&self) -> PermissionState {
        self.entry.state()
    }

    /// Returns the winning key pattern.
    #[must_use]
    pub const fn key(&self) -> &PermissionKey {
        self.entry.key()
    }

    /// Returns the winning context constraint.
    #[must_use]
    pub const fn context(&self) -> &PermissionRuleContext {
        self.entry.context()
    }

    /// Returns the key-specificity rank used during resolution.
    #[must_use]
    pub const fn key_specificity(&self) -> usize {
        self.key_specificity
    }

    /// Returns the context-specificity rank used during resolution.
    #[must_use]
    pub const fn context_specificity(&self) -> usize {
        self.context_specificity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PermissionCandidate {
    entry_index: usize,
    key_specificity: usize,
    context_specificity: usize,
    source: PermissionResolutionSource,
    state: PermissionState,
}

impl PermissionCandidate {
    const fn order(&self) -> (usize, usize, usize, i32) {
        (
            self.key_specificity,
            self.context_specificity,
            self.source.rank(),
            self.source.tie_priority(),
        )
    }
}

fn push_candidate(
    best: &mut Option<PermissionCandidate>,
    entry_index: usize,
    key_specificity: usize,
    context_specificity: usize,
    source: &PermissionResolutionSource,
    state: PermissionState,
) {
    let candidate = PermissionCandidate {
        entry_index,
        key_specificity,
        context_specificity,
        source: source.clone(),
        state,
    };
    match best {
        None => *best = Some(candidate),
        Some(current) if candidate.order() > current.order() => *best = Some(candidate),
        Some(current)
            if candidate.order() == current.order()
                && current.state == PermissionState::Allow
                && state == PermissionState::Deny =>
        {
            *best = Some(candidate);
        }
        _ => {}
    }
}
