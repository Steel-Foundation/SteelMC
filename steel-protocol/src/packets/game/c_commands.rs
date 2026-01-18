use std::borrow::Cow;
use std::io::{Result, Write};

use steel_macros::{ClientPacket, WriteTo};
#[allow(unused_imports)]
use steel_registry::packets::play::C_COMMANDS;
use steel_utils::{
    codec::VarInt,
    serial::{PrefixedWrite, WriteTo},
};

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_COMMANDS)]
pub struct CCommands {
    pub nodes: Vec<CommandNode>,
    #[write(as = VarInt)]
    pub root_index: i32,
}

pub enum CommandNode {
    Root {
        children: Vec<i32>,
    },
    Literal {
        children: Vec<i32>,
        redirects_to: Option<i32>,
        name: Cow<'static, str>,
        is_executable: bool,
    },
    Argument {
        children: Vec<i32>,
        redirects_to: Option<i32>,
        name: Cow<'static, str>,
        is_executable: bool,
        parser: ArgumentType,
        suggestions_type: Option<SuggestionType>,
    },
}

impl CommandNode {
    const FLAG_IS_EXECUTABLE: u8 = 4;
    const FLAG_HAS_REDIRECT: u8 = 8;
    const FLAG_HAS_SUGGESTION_TYPE: u8 = 16;

    pub fn new_root() -> Self {
        Self::Root {
            children: Vec::new(),
        }
    }

    pub fn new_literal(info: CommandNodeInfo, name: impl Into<Cow<'static, str>>) -> Self {
        Self::Literal {
            children: info.children,
            name: name.into(),
            is_executable: info.is_executable,
            redirects_to: info.redirects_to,
        }
    }

    pub fn new_argument(
        info: CommandNodeInfo,
        name: impl Into<Cow<'static, str>>,
        argument: (ArgumentType, Option<SuggestionType>),
    ) -> Self {
        Self::Argument {
            children: info.children,
            name: name.into(),
            is_executable: info.is_executable,
            redirects_to: info.redirects_to,
            parser: argument.0,
            suggestions_type: argument.1,
        }
    }

    fn flags(&self) -> u8 {
        let (mut flags, is_executable, has_redirect, has_suggestions_type) = match self {
            CommandNode::Root { .. } => (0, false, false, false),
            CommandNode::Literal {
                is_executable,
                redirects_to,
                ..
            } => (1, *is_executable, redirects_to.is_some(), false),
            CommandNode::Argument {
                is_executable,
                redirects_to: r,
                suggestions_type,
                ..
            } => (2, *is_executable, r.is_some(), suggestions_type.is_some()),
        };

        if is_executable {
            flags |= Self::FLAG_IS_EXECUTABLE
        }
        if has_redirect {
            flags |= Self::FLAG_HAS_REDIRECT
        }
        if has_suggestions_type {
            flags |= Self::FLAG_HAS_SUGGESTION_TYPE
        }
        flags
    }

    pub fn set_children(&mut self, children: Vec<i32>) {
        match self {
            CommandNode::Root { children: c } => *c = children,
            CommandNode::Literal { children: c, .. } => *c = children,
            CommandNode::Argument { children: c, .. } => *c = children,
        }
    }

    fn children(&self) -> &[i32] {
        match self {
            CommandNode::Root { children } => children,
            CommandNode::Literal { children, .. } => children,
            CommandNode::Argument { children, .. } => children,
        }
    }

    fn redirects_to(&self) -> &Option<i32> {
        match self {
            CommandNode::Root { .. } => &None,
            CommandNode::Literal { redirects_to, .. } => redirects_to,
            CommandNode::Argument { redirects_to, .. } => redirects_to,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            CommandNode::Root { .. } => None,
            CommandNode::Literal { name, .. } => Some(name),
            CommandNode::Argument { name, .. } => Some(name),
        }
    }
}

impl WriteTo for CommandNode {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        writer.write_all(&self.flags().to_be_bytes())?;
        let children = self.children();
        VarInt(children.len() as i32).write(writer)?;
        for child in children {
            VarInt(*child).write(writer)?;
        }

        if let Some(redirects_to) = self.redirects_to() {
            VarInt(*redirects_to).write(writer)?;
        }

        if let Some(name) = self.name() {
            name.write_prefixed::<VarInt>(writer)?;
        }

        if let CommandNode::Argument {
            parser,
            suggestions_type,
            ..
        } = self
        {
            parser.write(writer)?;

            if let Some(suggestions_type) = suggestions_type {
                suggestions_type.as_str().write_prefixed::<VarInt>(writer)?;
            }
        }

        Ok(())
    }
}

pub struct CommandNodeInfo {
    children: Vec<i32>,
    is_executable: bool,
    redirects_to: Option<i32>,
}

impl CommandNodeInfo {
    pub fn new(children: Vec<i32>) -> Self {
        Self {
            children,
            is_executable: false,
            redirects_to: None,
        }
    }

    pub fn new_executable() -> Self {
        Self {
            children: Vec::new(),
            is_executable: true,
            redirects_to: None,
        }
    }

    pub fn new_redirect(redirects_to: i32) -> Self {
        Self {
            children: Vec::new(),
            is_executable: false,
            redirects_to: Some(redirects_to),
        }
    }

    pub fn chain(mut self, mut other: Self) -> Self {
        self.children.append(&mut other.children);
        self.is_executable |= other.is_executable;
        self
    }
}

#[repr(u32)]
pub enum ArgumentType {
    Bool,
    Float {
        min: Option<f32>,
        max: Option<f32>,
    },
    Double {
        min: Option<f64>,
        max: Option<f64>,
    },
    Integer {
        min: Option<i32>,
        max: Option<i32>,
    },
    Long {
        min: Option<i64>,
        max: Option<i64>,
    },
    String {
        behavior: ArgumentStringTypeBehavior,
    },
    Entity {
        flags: u8,
    },
    GameProfile,
    BlockPos,
    ColumnPos,
    Vec3,
    Vec2,
    BlockState,
    BlockPredicate,
    ItemStack,
    ItemPredicate,
    Color,
    HexColor,
    Component,
    Style,
    Message,
    Nbt,
    NbtTag,
    NbtPath,
    Objective,
    ObjectiveCriteria,
    Operation,
    Particle,
    Angle,
    Rotation,
    ScoreboardSlot,
    ScoreHolder {
        flags: u8,
    },
    Swizzle,
    Team,
    ItemSlot,
    ItemSlots,
    ResourceLocation,
    Function,
    EntityAnchor,
    IntRange,
    FloatRange,
    Dimension,
    Gamemode,
    Time {
        min: i32,
    },
    ResourceOrTag {
        identifier: &'static str,
    },
    ResourceOrTagKey {
        identifier: &'static str,
    },
    Resource {
        identifier: &'static str,
    },
    ResourceKey {
        identifier: &'static str,
    },
    TemplateMirror,
    TemplateRotation,
    Heightmap,
    LootTable,
    LootPredicate,
    LootModifier,
    Dialog,
    Uuid,
}

#[derive(Debug, Clone, Copy)]
pub enum ArgumentStringTypeBehavior {
    SingleWord,
    QuotablePhrase,
    GreedyPhrase,
}

impl WriteTo for ArgumentType {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        // Safety: Since Self is repr(u32), it is guaranteed to hold the discriminant in the first 4 bytes
        // See https://doc.rust-lang.org/reference/items/enumerations.html#pointer-casting
        let id = unsafe { *(self as *const Self as *const i32) };
        VarInt(id).write(writer)?;

        match self {
            Self::Float { min, max } => Self::write_min_max(*min, *max, writer),
            Self::Double { min, max } => Self::write_min_max(*min, *max, writer),
            Self::Integer { min, max } => Self::write_min_max(*min, *max, writer),
            Self::Long { min, max } => Self::write_min_max(*min, *max, writer),
            Self::String { behavior } => {
                let i = match behavior {
                    ArgumentStringTypeBehavior::SingleWord => 0,
                    ArgumentStringTypeBehavior::QuotablePhrase => 1,
                    ArgumentStringTypeBehavior::GreedyPhrase => 2,
                };
                VarInt(i).write(writer)
            }
            Self::Entity { flags } => flags.write(writer),
            Self::ScoreHolder { flags } => flags.write(writer),
            Self::Time { min } => min.write(writer),
            Self::ResourceOrTag { identifier } => identifier.write_prefixed::<VarInt>(writer),
            Self::ResourceOrTagKey { identifier } => identifier.write_prefixed::<VarInt>(writer),
            Self::Resource { identifier } => identifier.write_prefixed::<VarInt>(writer),
            Self::ResourceKey { identifier } => identifier.write_prefixed::<VarInt>(writer),
            _ => Ok(()),
        }
    }
}

impl ArgumentType {
    fn write_min_max<T: WriteTo>(
        min: Option<T>,
        max: Option<T>,
        writer: &mut impl Write,
    ) -> Result<()> {
        // none = 0
        // min = 1
        // max = 2
        // min & max = 3
        (min.is_some() as u8 + max.is_some() as u8 + max.is_some() as u8).write(writer)?;

        if let Some(min) = min {
            min.write(writer)?;
        }
        if let Some(max) = max {
            max.write(writer)?;
        }

        Ok(())
    }
}

pub enum SuggestionType {
    AskServer,
    AllRecipes,
    AvailableSounds,
    SummonableEntities,
}

impl SuggestionType {
    fn as_str(&self) -> &str {
        match self {
            SuggestionType::AskServer => "minecraft:ask_server",
            SuggestionType::AllRecipes => "minecraft:all_recipes",
            SuggestionType::AvailableSounds => "minecraft:available_sounds",
            SuggestionType::SummonableEntities => "minecraft:summonable_entities",
        }
    }
}
