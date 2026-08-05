use std::borrow::Cow;
use std::io::{Result, Write};

use steel_macros::{ClientPacket, WriteTo};
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
        is_restricted: bool,
    },
    Argument {
        children: Vec<i32>,
        redirects_to: Option<i32>,
        name: Cow<'static, str>,
        is_executable: bool,
        is_restricted: bool,
        parser: ArgumentType,
        suggestions_type: Option<SuggestionType>,
    },
}

impl CommandNode {
    const FLAG_IS_EXECUTABLE: u8 = 4;
    const FLAG_HAS_REDIRECT: u8 = 8;
    const FLAG_HAS_SUGGESTION_TYPE: u8 = 16;
    const FLAG_IS_RESTRICTED: u8 = 32;

    #[must_use]
    pub const fn new_root() -> Self {
        Self::Root {
            children: Vec::new(),
        }
    }

    pub fn new_literal(info: CommandNodeInfo, name: impl Into<Cow<'static, str>>) -> Self {
        Self::Literal {
            children: info.children,
            name: name.into(),
            is_executable: info.is_executable,
            is_restricted: info.is_restricted,
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
            is_restricted: info.is_restricted,
            redirects_to: info.redirects_to,
            parser: argument.0,
            suggestions_type: argument.1,
        }
    }

    const fn flags(&self) -> u8 {
        let (mut flags, is_executable, has_redirect, has_suggestions_type, is_restricted) =
            match self {
                CommandNode::Root { .. } => (0, false, false, false, false),
                CommandNode::Literal {
                    is_executable,
                    redirects_to,
                    is_restricted,
                    ..
                } => (
                    1,
                    *is_executable,
                    redirects_to.is_some(),
                    false,
                    *is_restricted,
                ),
                CommandNode::Argument {
                    is_executable,
                    redirects_to: r,
                    suggestions_type,
                    is_restricted,
                    ..
                } => (
                    2,
                    *is_executable,
                    r.is_some(),
                    suggestions_type.is_some(),
                    *is_restricted,
                ),
            };

        if is_executable {
            flags |= Self::FLAG_IS_EXECUTABLE;
        }
        if has_redirect {
            flags |= Self::FLAG_HAS_REDIRECT;
        }
        if has_suggestions_type {
            flags |= Self::FLAG_HAS_SUGGESTION_TYPE;
        }
        if is_restricted {
            flags |= Self::FLAG_IS_RESTRICTED;
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

    const fn redirects_to(&self) -> &Option<i32> {
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
    is_restricted: bool,
    redirects_to: Option<i32>,
}

impl CommandNodeInfo {
    #[must_use]
    pub const fn new(children: Vec<i32>) -> Self {
        Self {
            children,
            is_executable: false,
            is_restricted: false,
            redirects_to: None,
        }
    }

    #[must_use]
    pub const fn new_executable() -> Self {
        Self {
            children: Vec::new(),
            is_executable: true,
            is_restricted: false,
            redirects_to: None,
        }
    }

    #[must_use]
    pub const fn new_redirect(redirects_to: i32) -> Self {
        Self {
            children: Vec::new(),
            is_executable: false,
            is_restricted: false,
            redirects_to: Some(redirects_to),
        }
    }

    /// Marks this node as executable.
    #[must_use]
    pub const fn executable(mut self) -> Self {
        self.is_executable = true;
        self
    }

    /// Marks this node as requiring authorization on the server.
    #[must_use]
    pub const fn restricted(mut self) -> Self {
        self.is_restricted = true;
        self
    }

    /// Adds a redirect to another serialized command node.
    #[must_use]
    pub const fn redirect(mut self, redirects_to: i32) -> Self {
        self.redirects_to = Some(redirects_to);
        self
    }

    #[must_use]
    pub fn chain(mut self, mut other: Self) -> Self {
        self.children.append(&mut other.children);
        self.is_executable |= other.is_executable;
        self.is_restricted |= other.is_restricted;
        self
    }
}

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
    ResourceSelector {
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

impl ArgumentType {
    const fn discriminant(&self) -> i32 {
        match self {
            Self::Bool => 0,
            Self::Float { .. } => 1,
            Self::Double { .. } => 2,
            Self::Integer { .. } => 3,
            Self::Long { .. } => 4,
            Self::String { .. } => 5,
            Self::Entity { .. } => 6,
            Self::GameProfile => 7,
            Self::BlockPos => 8,
            Self::ColumnPos => 9,
            Self::Vec3 => 10,
            Self::Vec2 => 11,
            Self::BlockState => 12,
            Self::BlockPredicate => 13,
            Self::ItemStack => 14,
            Self::ItemPredicate => 15,
            Self::Color => 16,
            Self::HexColor => 17,
            Self::Component => 18,
            Self::Style => 19,
            Self::Message => 20,
            Self::Nbt => 21,
            Self::NbtTag => 22,
            Self::NbtPath => 23,
            Self::Objective => 24,
            Self::ObjectiveCriteria => 25,
            Self::Operation => 26,
            Self::Particle => 27,
            Self::Angle => 28,
            Self::Rotation => 29,
            Self::ScoreboardSlot => 30,
            Self::ScoreHolder { .. } => 31,
            Self::Swizzle => 32,
            Self::Team => 33,
            Self::ItemSlot => 34,
            Self::ItemSlots => 35,
            Self::ResourceLocation => 36,
            Self::Function => 37,
            Self::EntityAnchor => 38,
            Self::IntRange => 39,
            Self::FloatRange => 40,
            Self::Dimension => 41,
            Self::Gamemode => 42,
            Self::Time { .. } => 43,
            Self::ResourceOrTag { .. } => 44,
            Self::ResourceOrTagKey { .. } => 45,
            Self::Resource { .. } => 46,
            Self::ResourceKey { .. } => 47,
            Self::ResourceSelector { .. } => 48,
            Self::TemplateMirror => 49,
            Self::TemplateRotation => 50,
            Self::Heightmap => 51,
            Self::LootTable => 52,
            Self::LootPredicate => 53,
            Self::LootModifier => 54,
            Self::Dialog => 55,
            Self::Uuid => 56,
        }
    }
}

impl WriteTo for ArgumentType {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.discriminant()).write(writer)?;

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
            Self::ResourceSelector { identifier } => identifier.write_prefixed::<VarInt>(writer),
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
        (u8::from(min.is_some()) + u8::from(max.is_some()) + u8::from(max.is_some()))
            .write(writer)?;

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
    const fn as_str(&self) -> &str {
        match self {
            SuggestionType::AskServer => "minecraft:ask_server",
            SuggestionType::AllRecipes => "minecraft:all_recipes",
            SuggestionType::AvailableSounds => "minecraft:available_sounds",
            SuggestionType::SummonableEntities => "minecraft:summonable_entities",
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo;

    use super::{ArgumentType, CommandNode, CommandNodeInfo};

    #[test]
    fn restricted_nodes_set_the_26_2_protocol_flag() {
        let node = CommandNode::new_literal(CommandNodeInfo::new(Vec::new()).restricted(), "op");
        let mut encoded = Vec::new();

        assert!(node.write(&mut encoded).is_ok());
        assert_eq!(encoded.first().copied(), Some(1 | 32));
    }

    #[test]
    fn command_node_flags_can_be_combined() {
        let info = CommandNodeInfo::new(Vec::new())
            .executable()
            .restricted()
            .redirect(0);
        let node = CommandNode::new_literal(info, "alias");
        let mut encoded = Vec::new();

        assert!(node.write(&mut encoded).is_ok());
        assert_eq!(encoded.first().copied(), Some(1 | 4 | 8 | 32));
    }

    #[test]
    fn command_argument_tail_uses_the_26_2_registry_ids() {
        for (argument, expected) in [
            (ArgumentType::TemplateMirror, 49),
            (ArgumentType::TemplateRotation, 50),
            (ArgumentType::Heightmap, 51),
            (ArgumentType::LootTable, 52),
            (ArgumentType::LootPredicate, 53),
            (ArgumentType::LootModifier, 54),
            (ArgumentType::Dialog, 55),
            (ArgumentType::Uuid, 56),
        ] {
            let mut encoded = Vec::new();
            assert!(argument.write(&mut encoded).is_ok());
            assert_eq!(encoded, [expected]);
        }
    }

    #[test]
    fn resource_selector_writes_its_registry_key() {
        let mut encoded = Vec::new();
        let argument = ArgumentType::ResourceSelector {
            identifier: "minecraft:test_instance",
        };

        assert!(argument.write(&mut encoded).is_ok());
        assert_eq!(&encoded[..2], [48, 23]);
        assert_eq!(&encoded[2..], b"minecraft:test_instance");
    }
}
