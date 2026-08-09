use serde::Serialize;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::status::C_STATUS_RESPONSE;

#[derive(Serialize, Clone, Debug)]
pub struct Sample {
    /// The player's name.
    pub name: String,
    /// The player's UUID.
    pub id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Players {
    pub max: i32,
    pub online: i32,
    pub sample: Vec<Sample>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Version {
    pub name: &'static str,
    pub protocol: i32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub description: String,
    pub players: Option<Players>,
    pub version: Option<Version>,
    pub favicon: Option<String>,
    #[serde(rename = "enforcesSecureChat")]
    pub enforce_secure_chat: bool,
}

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Status = C_STATUS_RESPONSE)]
pub struct CStatusResponse {
    #[write(as = Json)]
    status: Status,
}

impl CStatusResponse {
    #[must_use]
    pub const fn new(status: Status) -> Self {
        Self { status }
    }
}

#[cfg(test)]
mod tests {
    use super::Status;

    #[test]
    fn secure_chat_enforcement_uses_vanilla_json_name() {
        let status = Status {
            description: String::new(),
            players: None,
            version: None,
            favicon: None,
            enforce_secure_chat: true,
        };
        let json = serde_json::to_value(status).expect("status should serialize");

        assert_eq!(
            json.get("enforcesSecureChat"),
            Some(&serde_json::json!(true))
        );
        assert!(json.get("enforce_secure_chat").is_none());
    }
}
