use crate::advancement::criterion::AnyCriterion;
use crate::advancement::display::DisplayInfo;
use crate::loot_table::LootTableRef;
use crate::recipe::UntypedRecipeRef;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::{Ordering, PartialEq};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::io::Write;
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedWrite, WriteTo};
use steel_utils::translations::CHAT_SQUARE_BRACKETS;
use text_components::interactivity::HoverEvent;
use text_components::{Modifier, TextComponent};

pub mod criterion;
pub mod display;
pub mod positioner;
pub mod registry;

#[derive(Default)]
pub struct Advancement {
    pub key: Identifier,
    pub parent: Option<Identifier>,
    pub criteria: BTreeMap<String, Box<dyn AnyCriterion>>,
    pub display: Option<DisplayInfo>,
    pub send_telemetry_event: bool,
    pub requirements: AdvancementRequirement,
    pub rewards: AdvancementRewards,
}

impl Advancement {
    #[inline]
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    pub fn name(&self) -> TextComponent {
        self.decorate_name()
            .unwrap_or(TextComponent::plain(self.key.to_string()))
    }

    pub fn decorate_name(&self) -> Option<TextComponent> {
        match &self.display {
            Some(display) => {
                let color = display.frame_type.color();
                let over = display
                    .title
                    .clone()
                    .color(color.clone())
                    .add_child("\n")
                    .add_child(display.description.clone());
                let text = display
                    .title
                    .clone()
                    .hover_event(HoverEvent::show_text(over));
                Some(TextComponent::translated(CHAT_SQUARE_BRACKETS.message([text])).color(color))
            }
            None => None,
        }
    }
}

impl Hash for Advancement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl PartialEq for Advancement {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for Advancement {}

impl Display for Advancement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.key)
    }
}

impl Debug for Advancement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.key)
    }
}

impl PartialOrd for Advancement {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Advancement {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl WriteTo for Advancement {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.key.write(writer)?;
        self.parent.write(writer)?;
        self.display.write(writer)?;
        self.requirements.write(writer)?;
        self.send_telemetry_event.write(writer)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AdvancementRequirement {
    pub requirements: Vec<Vec<Cow<'static, str>>>,
}

impl AdvancementRequirement {
    #[must_use]
    pub fn names(&self) -> Vec<Cow<'static, str>> {
        self.requirements.iter().flatten().cloned().collect()
    }

    /// test if the requirements is complete
    pub fn test(&self, predicate: impl Fn(&str) -> bool) -> bool {
        if self.requirements.is_empty() {
            false
        } else {
            for requirement in &self.requirements {
                if !Self::any_match(requirement, &predicate) {
                    return false;
                }
            }
            true
        }
    }

    /// check if any test pass
    fn any_match(requirements: &Vec<Cow<'static, str>>, predicate: impl Fn(&str) -> bool) -> bool {
        for requirement in requirements {
            if predicate(requirement) {
                return true;
            }
        }
        false
    }
}

impl WriteTo for AdvancementRequirement {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        VarInt(self.requirements.len() as i32).write(writer)?;
        for requirement in &self.requirements {
            VarInt(requirement.len() as i32).write(writer)?;
            for req in requirement {
                req.to_string().write_prefixed::<VarInt>(writer)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct AdvancementRewards {
    pub experience: i32,
    pub loots: Vec<LootTableRef>,
    pub recipes: Vec<UntypedRecipeRef>,
    pub function: Option<Identifier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancementProgressData {
    pub id: Identifier,
    pub progress: Vec<Criteria>,
}

impl WriteTo for AdvancementProgressData {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.id.write(writer)?;
        self.progress.write(writer)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Criteria {
    pub criterion_id: Cow<'static, str>,
    pub achieve_date: Option<i64>,
}

impl WriteTo for Criteria {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.criterion_id
            .to_string()
            .write_prefixed::<VarInt>(writer)?;
        self.achieve_date.write(writer)?;
        Ok(())
    }
}
