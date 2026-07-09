use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tokio::io;
use uuid::Uuid;

use crate::permission::{
    PermissionEntry, PermissionRuleExpression, PermissionSegment, PermissionSet, PermissionState,
    PermissionSubjectIndex, PermissionSubjectState,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PlayerPermissionsFile {
    pub(super) players: BTreeMap<String, PlayerPermissionEntryFile>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PlayerPermissionEntryFile {
    pub(super) groups: Vec<String>,
    pub(super) allow: Vec<String>,
    pub(super) deny: Vec<String>,
}

impl PlayerPermissionsFile {
    pub(super) fn validate(&self) -> io::Result<()> {
        for (uuid, entry) in &self.players {
            let uuid = parse_uuid(uuid)?;
            entry.validate(uuid)?;
        }
        Ok(())
    }

    pub(super) fn subject(&self, uuid: Uuid) -> io::Result<Option<PermissionSubjectState>> {
        self.players
            .get(&uuid.to_string())
            .map(|entry| entry.to_subject_state(uuid))
            .transpose()
    }

    pub(super) fn into_subject_index(self) -> io::Result<PermissionSubjectIndex> {
        let mut subjects = PermissionSubjectIndex::new();
        for (uuid, entry) in self.players {
            let uuid = parse_uuid(&uuid)?;
            subjects.set(uuid, entry.into_subject_state(uuid)?);
        }
        Ok(subjects)
    }
}

impl PlayerPermissionEntryFile {
    fn validate(&self, uuid: Uuid) -> io::Result<()> {
        validate_groups(uuid, &self.groups)?;
        for expression in &self.allow {
            parse_permission_expression(uuid, expression, "allow")?;
        }
        for expression in &self.deny {
            parse_permission_expression(uuid, expression, "deny")?;
        }
        Ok(())
    }

    fn from_subject_state(state: &PermissionSubjectState) -> Self {
        let mut allow = Vec::new();
        let mut deny = Vec::new();
        for entry in state.overrides().entries() {
            let expression =
                PermissionRuleExpression::new(entry.key().clone(), entry.context().clone())
                    .to_string();
            match entry.state() {
                PermissionState::Allow => allow.push(expression),
                PermissionState::Deny => deny.push(expression),
            }
        }
        Self {
            groups: state.groups().to_vec(),
            allow,
            deny,
        }
    }

    fn to_subject_state(&self, uuid: Uuid) -> io::Result<PermissionSubjectState> {
        self.clone().into_subject_state(uuid)
    }

    fn into_subject_state(self, uuid: Uuid) -> io::Result<PermissionSubjectState> {
        validate_groups(uuid, &self.groups)?;
        let mut overrides = PermissionSet::new();
        for expression in self.allow {
            let expression = parse_permission_expression(uuid, &expression, "allow")?;
            let (key, context) = expression.into_parts();
            overrides.push(PermissionEntry::allow_with_context(key, context));
        }
        for expression in self.deny {
            let expression = parse_permission_expression(uuid, &expression, "deny")?;
            let (key, context) = expression.into_parts();
            overrides.push(PermissionEntry::deny_with_context(key, context));
        }
        Ok(PermissionSubjectState::new(self.groups, overrides))
    }
}

fn parse_uuid(value: &str) -> io::Result<Uuid> {
    Uuid::parse_str(value).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid player permission UUID '{value}': {error}"),
        )
    })
}

fn validate_groups(uuid: Uuid, groups: &[String]) -> io::Result<()> {
    for group in groups {
        PermissionSegment::parse(group.as_str()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid permission group '{group}' for {uuid}: {error}"),
            )
        })?;
    }
    Ok(())
}

fn parse_permission_expression(
    uuid: Uuid,
    expression: &str,
    state: &str,
) -> io::Result<PermissionRuleExpression> {
    PermissionRuleExpression::parse(expression).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {state} permission expression for {uuid}: {error}"),
        )
    })
}

pub(super) fn set_permission_subject(
    file: &mut PlayerPermissionsFile,
    uuid: Uuid,
    state: &PermissionSubjectState,
) {
    if state.is_empty() {
        file.players.remove(&uuid.to_string());
        return;
    }
    file.players.insert(
        uuid.to_string(),
        PlayerPermissionEntryFile::from_subject_state(state),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::{PermissionKey, PermissionRuleContext};

    fn key(value: &str) -> PermissionKey {
        match PermissionKey::parse(value) {
            Ok(key) => key,
            Err(error) => panic!("test permission key should parse: {error}"),
        }
    }

    #[test]
    fn subject_file_round_trip_preserves_groups_and_contextual_rules() {
        let uuid = Uuid::from_u128(1);
        let mut overrides = PermissionSet::new();
        overrides.allow(key("steel.fly"));
        overrides.deny_in(
            key("steel.build"),
            PermissionRuleContext::domain("survival"),
        );
        let state = PermissionSubjectState::new(vec!["retired_group".to_owned()], overrides);
        let mut file = PlayerPermissionsFile::default();
        set_permission_subject(&mut file, uuid, &state);

        let serialized = match toml::to_string_pretty(&file) {
            Ok(serialized) => serialized,
            Err(error) => panic!("subject file should serialize: {error}"),
        };
        let parsed = match toml::from_str::<PlayerPermissionsFile>(&serialized) {
            Ok(parsed) => parsed,
            Err(error) => panic!("subject file should parse: {error}"),
        };
        let parsed = match parsed.subject(uuid) {
            Ok(Some(parsed)) => parsed,
            Ok(None) => panic!("subject should exist"),
            Err(error) => panic!("subject should validate: {error}"),
        };

        assert_eq!(parsed, state);
    }

    #[test]
    fn subject_file_rejects_invalid_group_names() {
        let uuid = Uuid::from_u128(2);
        let mut file = PlayerPermissionsFile::default();
        file.players.insert(
            uuid.to_string(),
            PlayerPermissionEntryFile {
                groups: vec!["Admin Group".to_owned()],
                ..PlayerPermissionEntryFile::default()
            },
        );

        let error = file.validate();
        assert!(error.is_err_and(|error| {
            error
                .to_string()
                .contains("invalid permission group 'Admin Group'")
        }));
    }

    #[test]
    fn empty_subject_state_removes_the_file_entry() {
        let uuid = Uuid::from_u128(3);
        let mut file = PlayerPermissionsFile::default();
        file.players
            .insert(uuid.to_string(), PlayerPermissionEntryFile::default());

        set_permission_subject(&mut file, uuid, &PermissionSubjectState::default());

        assert!(file.players.is_empty());
    }
}
