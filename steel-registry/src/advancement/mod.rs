use crate::advancement::criterion::AnyCriterion;
use crate::advancement::display::DisplayInfo;
use crate::loot_table::LootTableRef;
use crate::recipe::UntypedRecipeRef;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::PartialEq;
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::io::Write;
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedWrite, WriteTo};

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

impl WriteTo for Advancement {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.key.write(writer)?;
        self.parent.write(writer)?;
        self.display.write(writer)?;
        self.requirements.write(writer)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AdvancementRequirement {
    pub requirements: Vec<Vec<Cow<'static, str>>>,
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

impl Advancement {
    #[inline]
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent.is_none()
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
        Cow::Borrowed("hello");
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
