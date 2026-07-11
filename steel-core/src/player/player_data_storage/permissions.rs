use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tokio::io;
use toml::ser::Error as TomlSerializeError;
use uuid::Uuid;

use crate::permission::{
    PermissionEntry, PermissionMetadataEntry, PermissionMetadataExpression, PermissionMetadataSet,
    PermissionMetadataValue, PermissionRuleExpression, PermissionSegment, PermissionSet,
    PermissionState, PermissionSubjectIndex, PermissionSubjectState,
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
    pub(super) metadata: Vec<PlayerPermissionMetadataEntryFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct PlayerPermissionMetadataEntryFile {
    pub(super) key: String,
    pub(super) value: PermissionMetadataValue,
}

impl PlayerPermissionsFile {
    pub(super) fn from_subject_index(subjects: &PermissionSubjectIndex) -> Self {
        let mut file = Self::default();
        for (uuid, state) in subjects.entries() {
            set_permission_subject(&mut file, uuid, state);
        }
        file
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        for (uuid, entry) in &self.players {
            let uuid = parse_uuid(uuid)?;
            entry.validate(uuid)?;
        }
        Ok(())
    }

    pub(super) fn into_subject_index(self) -> io::Result<PermissionSubjectIndex> {
        let mut subjects = PermissionSubjectIndex::new();
        for (uuid_text, entry) in self.players {
            let uuid = parse_uuid(&uuid_text)?;
            if subjects.get(uuid).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate player permission UUID '{uuid_text}' resolves to {uuid}"),
                ));
            }
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
        for entry in &self.metadata {
            parse_metadata_expression(uuid, &entry.key)?;
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
            metadata: state
                .metadata_overrides()
                .entries()
                .iter()
                .map(|entry| PlayerPermissionMetadataEntryFile {
                    key: PermissionMetadataExpression::new(
                        entry.key().clone(),
                        entry.context().clone(),
                    )
                    .to_string(),
                    value: entry.value().clone(),
                })
                .collect(),
        }
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
        let mut metadata = PermissionMetadataSet::new();
        for entry in self.metadata {
            let expression = parse_metadata_expression(uuid, &entry.key)?;
            let (key, context) = expression.into_parts();
            metadata.push(PermissionMetadataEntry::new_with_context(
                key,
                context,
                entry.value,
            ));
        }
        Ok(PermissionSubjectState::new_with_metadata(
            self.groups,
            overrides,
            metadata,
        ))
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

fn parse_metadata_expression(
    uuid: Uuid,
    expression: &str,
) -> io::Result<PermissionMetadataExpression> {
    PermissionMetadataExpression::parse(expression).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid permission metadata expression for {uuid}: {error}"),
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

pub(super) fn serialize_player_permissions_file(
    file: &PlayerPermissionsFile,
) -> Result<String, TomlSerializeError> {
    let mut output = String::new();
    if file.players.is_empty() {
        output.push_str("players = {}\n");
        return Ok(output);
    }

    for (uuid, entry) in &file.players {
        output.push_str("[players.");
        output.push_str(&toml_value(uuid)?);
        output.push_str("]\n");
        push_player_permission_entry(&mut output, entry)?;
        output.push('\n');
    }
    Ok(output)
}

fn push_player_permission_entry(
    output: &mut String,
    entry: &PlayerPermissionEntryFile,
) -> Result<(), TomlSerializeError> {
    push_string_array_field(output, "groups", &entry.groups)?;
    push_string_array_field(output, "allow", &entry.allow)?;
    push_string_array_field(output, "deny", &entry.deny)?;
    push_permission_metadata_entries(output, &entry.metadata)
}

fn push_string_array_field(
    output: &mut String,
    key: &str,
    values: &[String],
) -> Result<(), TomlSerializeError> {
    if values.is_empty() {
        output.push_str(key);
        output.push_str(" = []\n");
        return Ok(());
    }

    output.push_str(key);
    output.push_str(" = [\n");
    for value in values {
        output.push_str("    ");
        output.push_str(&toml_value(value)?);
        output.push_str(",\n");
    }
    output.push_str("]\n");
    Ok(())
}

fn push_permission_metadata_entries(
    output: &mut String,
    metadata: &[PlayerPermissionMetadataEntryFile],
) -> Result<(), TomlSerializeError> {
    if metadata.is_empty() {
        output.push_str("metadata = []\n");
        return Ok(());
    }

    output.push_str("metadata = [\n");
    for entry in metadata {
        output.push_str("    { key = ");
        output.push_str(&toml_value(&entry.key)?);
        output.push_str(", value = ");
        output.push_str(&permission_metadata_value_toml(&entry.value)?);
        output.push_str(" },\n");
    }
    output.push_str("]\n");
    Ok(())
}

fn toml_value<T: Serialize + ?Sized>(value: &T) -> Result<String, TomlSerializeError> {
    #[derive(Serialize)]
    struct Field<'a, T: Serialize + ?Sized> {
        value: &'a T,
    }

    let serialized = toml::to_string(&Field { value })?;
    let serialized = serialized.trim_end();
    Ok(serialized
        .strip_prefix("value = ")
        .unwrap_or(serialized)
        .to_owned())
}

fn permission_metadata_value_toml(
    value: &PermissionMetadataValue,
) -> Result<String, TomlSerializeError> {
    match value {
        PermissionMetadataValue::Bool(value) => toml_value(value),
        PermissionMetadataValue::Integer(value) => toml_value(value),
        PermissionMetadataValue::String(value) => toml_value(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::{
        PermissionKey, PermissionMetadataValue, PermissionRuleContext,
        parse_permission_metadata_key,
    };

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
            PermissionRuleContext::domain("survival")
                .unwrap_or_else(|error| panic!("test domain should parse: {error}")),
        );
        let metadata_key = match parse_permission_metadata_key("plugin:max_homes") {
            Ok(key) => key,
            Err(error) => panic!("test metadata key should parse: {error}"),
        };
        let mut metadata = PermissionMetadataSet::new();
        metadata.set_in(
            metadata_key,
            PermissionRuleContext::domain("survival")
                .unwrap_or_else(|error| panic!("test domain should parse: {error}")),
            PermissionMetadataValue::Integer(5),
        );
        let state = PermissionSubjectState::new_with_metadata(
            vec!["retired_group".to_owned()],
            overrides,
            metadata,
        );
        let mut file = PlayerPermissionsFile::default();
        set_permission_subject(&mut file, uuid, &state);

        let serialized = match serialize_player_permissions_file(&file) {
            Ok(serialized) => serialized,
            Err(error) => panic!("subject file should serialize: {error}"),
        };

        assert!(serialized.contains("{ key = \"plugin:max_homes{domain=survival}\", value = 5 }"));
        let parsed = match toml::from_str::<PlayerPermissionsFile>(&serialized) {
            Ok(parsed) => parsed,
            Err(error) => panic!("subject file should parse: {error}"),
        };
        let parsed = match parsed.into_subject_index() {
            Ok(parsed) => parsed,
            Err(error) => panic!("subjects should validate: {error}"),
        };
        let Some(parsed) = parsed.get(uuid) else {
            panic!("subject should exist");
        };

        assert_eq!(parsed, &state);
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

    #[test]
    fn subject_file_rejects_duplicate_uuid_spellings() {
        let uuid = Uuid::from_u128(4);
        let mut file = PlayerPermissionsFile::default();
        file.players.insert(
            uuid.to_string(),
            PlayerPermissionEntryFile {
                groups: vec!["op".to_owned()],
                ..PlayerPermissionEntryFile::default()
            },
        );
        file.players.insert(
            uuid.simple().to_string(),
            PlayerPermissionEntryFile {
                groups: vec!["builder".to_owned()],
                ..PlayerPermissionEntryFile::default()
            },
        );

        let error = file.into_subject_index();

        assert!(error.is_err_and(|error| {
            error
                .to_string()
                .contains("duplicate player permission UUID")
        }));
    }
}
