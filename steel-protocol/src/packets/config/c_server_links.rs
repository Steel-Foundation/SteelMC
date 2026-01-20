use serde::{Deserialize, Serialize};
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::config::C_SERVER_LINKS;
use steel_utils::codec::Or;
use steel_utils::text::TextComponent;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Config = C_SERVER_LINKS)]
pub struct CServerLinks {
    pub links: Vec<Link>,
}

#[derive(WriteTo, Clone, Copy, Debug, Serialize, Deserialize)]
#[write(as = VarInt)]
#[serde(rename_all = "snake_case")]
pub enum ServerLinksType {
    BugReport = 0,
    CommunityGuidelines = 1,
    Support = 2,
    Status = 3,
    Feedback = 4,
    Community = 5,
    Website = 6,
    Forums = 7,
    News = 8,
    Announcements = 9,
}

#[derive(WriteTo, Clone, Debug)]
pub struct Link {
    pub is_built_in: bool,
    pub label: Label,
    #[write(as = Prefixed(VarInt))]
    pub url: String,
}

impl Link {
    pub fn new(label: Label, url: String) -> Self {
        Self {
            is_built_in: label.is_left(),
            label,
            url,
        }
    }
}

/// Label can be either a built-in ServerLinksType (Left) or a custom `TextComponent` (Right).
/// The discriminant is stored separately in the `is_built_in` field of the Link struct.
pub type Label = Or<ServerLinksType, TextComponent>; // TextComponent goes here when uncommented
