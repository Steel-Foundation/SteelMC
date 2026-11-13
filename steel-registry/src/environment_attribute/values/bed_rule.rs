use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::{DowncastType, DowncastTypeKey};
use text_components::TextComponent;

/// How beds behave in an environment, as vanilla's `world.attribute.BedRule`.
///
/// Vanilla stores the error message as an optional component; every vanilla
/// rule uses a translation, so Steel keeps its key and a component builder.
#[derive(Debug, Clone)]
pub struct BedRule {
    pub can_set_spawn: BedRuleValue,
    pub can_sleep: BedRuleValue,
    pub destroy_on_use: bool,
    pub destroy_on_leave: bool,
    pub error_message_key: Option<&'static str>,
    pub error_message: Option<fn() -> TextComponent>,
}

/// Vanilla `BedRule.Rule`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BedRuleValue {
    Always,
    WhenDark,
    Never,
}

impl BedRuleValue {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::WhenDark => "when_dark",
            Self::Never => "never",
        }
    }
}

impl ToNbtTag for BedRuleValue {
    fn to_nbt_tag(self) -> NbtTag {
        self.as_str().to_nbt_tag()
    }
}

// SAFETY: This Steel-owned key uniquely identifies `BedRule` within the process.
unsafe impl DowncastType for BedRule {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:environment_attribute/bed_rule");
}
