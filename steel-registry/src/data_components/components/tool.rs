//! Tool component for mining speed and drop behavior.

use std::io::{Result, Write};
use std::str::FromStr;

use steel_utils::{
    BlockStateId, Identifier,
    hash::{ComponentHasher, HashComponent},
    serial::{ReadFrom, WriteTo},
};

use crate::REGISTRY;

/// A single rule within a Tool component.
/// Rules are evaluated in order; the first matching rule determines the speed/drop behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRule {
    /// The blocks this rule applies to (can be a tag like "#minecraft:mineable/pickaxe",
    /// a single block like "minecraft:cobweb", or a list of blocks).
    pub blocks: Vec<Identifier>,
    /// The mining speed for these blocks. If None, uses the tool's default_mining_speed.
    pub speed: Option<f32>,
    /// Whether the tool is "correct" for dropping items from these blocks.
    /// If None, falls back to the block's requiresCorrectToolForDrops property.
    pub correct_for_drops: Option<bool>,
}

impl ToolRule {
    /// Creates a rule that sets both mining speed and marks the tool as correct for drops.
    #[must_use]
    pub fn mines_and_drops(blocks: Vec<Identifier>, speed: f32) -> Self {
        Self {
            blocks,
            speed: Some(speed),
            correct_for_drops: Some(true),
        }
    }

    /// Creates a rule that explicitly denies drops (e.g., incorrect tool tier).
    #[must_use]
    pub fn denies_drops(blocks: Vec<Identifier>) -> Self {
        Self {
            blocks,
            speed: None,
            correct_for_drops: Some(false),
        }
    }

    /// Creates a rule that only overrides the mining speed.
    #[must_use]
    pub fn override_speed(blocks: Vec<Identifier>, speed: f32) -> Self {
        Self {
            blocks,
            speed: Some(speed),
            correct_for_drops: None,
        }
    }

    /// Checks if this rule matches a block state.
    /// Handles both direct block identifiers and block tags (prefixed with #).
    #[must_use]
    pub fn matches_block(&self, block_state_id: BlockStateId) -> bool {
        let Some(block) = REGISTRY.blocks.by_state_id(block_state_id) else {
            return false;
        };

        for block_id in &self.blocks {
            let id_str = format!("{}:{}", block_id.namespace, block_id.path);

            // Check if it's a tag reference (starts with #)
            if let Some(tag_str) = id_str.strip_prefix('#') {
                if let Ok(tag_id) = Identifier::from_str(tag_str)
                    && REGISTRY.blocks.is_in_tag(block, &tag_id)
                {
                    return true;
                }
            } else {
                // Direct block match
                if block.key == *block_id {
                    return true;
                }
            }
        }

        false
    }
}

/// The tool component data - defines mining speed and drop behavior for blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    /// Rules evaluated in order to determine mining speed and drop behavior.
    pub rules: Vec<ToolRule>,
    /// Default mining speed when no rule matches.
    pub default_mining_speed: f32,
    /// Damage to apply to the item per block mined.
    pub damage_per_block: i32,
    /// Whether the tool can destroy blocks in creative mode.
    pub can_destroy_blocks_in_creative: bool,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            default_mining_speed: 1.0,
            damage_per_block: 1,
            can_destroy_blocks_in_creative: true,
        }
    }
}

impl Tool {
    /// Returns the mining speed for a block state.
    /// Evaluates rules in order; returns the first matching rule's speed,
    /// or `default_mining_speed` if no rule matches.
    #[must_use]
    pub fn get_mining_speed(&self, block_state_id: BlockStateId) -> f32 {
        for rule in &self.rules {
            if let Some(speed) = rule.speed
                && rule.matches_block(block_state_id)
            {
                return speed;
            }
        }
        self.default_mining_speed
    }

    /// Returns true if this tool is "correct" for getting drops from the block.
    /// Evaluates rules in order; returns the first matching rule's `correct_for_drops`,
    /// or `false` if no rule explicitly matches.
    #[must_use]
    pub fn is_correct_for_drops(&self, block_state_id: BlockStateId) -> bool {
        for rule in &self.rules {
            if let Some(correct) = rule.correct_for_drops
                && rule.matches_block(block_state_id)
            {
                return correct;
            }
        }
        false
    }
}

impl WriteTo for Tool {
    fn write(&self, _writer: &mut impl Write) -> Result<()> {
        // TODO: Implement proper Tool serialization
        // Format: rules (list), default_mining_speed (float), damage_per_block (VarInt)
        Ok(())
    }
}

impl ReadFrom for Tool {
    fn read(_data: &mut std::io::Cursor<&[u8]>) -> Result<Self> {
        // TODO: Implement proper Tool deserialization
        Ok(Self::default())
    }
}

impl HashComponent for Tool {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Tool is hashed as a map with: rules, default_mining_speed, damage_per_block
        // For now, hash as empty map since full implementation requires proper codec
        hasher.start_map();
        // TODO: Add proper field hashing when Tool codec is implemented
        hasher.end_map();
    }
}

impl simdnbt::ToNbtTag for Tool {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

        let mut compound = NbtCompound::new();

        // Serialize rules
        let rules: Vec<NbtCompound> = self
            .rules
            .into_iter()
            .map(|rule| {
                let mut rule_compound = NbtCompound::new();

                // Blocks as a list of strings
                let blocks: Vec<NbtTag> = rule
                    .blocks
                    .into_iter()
                    .map(|id| NbtTag::String(id.to_string().into()))
                    .collect();
                rule_compound.insert(
                    "blocks",
                    NbtList::String(
                        blocks
                            .into_iter()
                            .filter_map(|t| match t {
                                NbtTag::String(s) => Some(s),
                                _ => None,
                            })
                            .collect(),
                    ),
                );

                if let Some(speed) = rule.speed {
                    rule_compound.insert("speed", speed);
                }
                if let Some(correct) = rule.correct_for_drops {
                    rule_compound.insert("correct_for_drops", i8::from(correct));
                }

                rule_compound
            })
            .collect();
        compound.insert("rules", NbtList::Compound(rules));

        compound.insert("default_mining_speed", self.default_mining_speed);
        compound.insert("damage_per_block", self.damage_per_block);
        compound.insert(
            "can_destroy_blocks_in_creative",
            i8::from(self.can_destroy_blocks_in_creative),
        );

        NbtTag::Compound(compound)
    }
}

impl simdnbt::FromNbtTag for Tool {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;

        // Parse rules
        let mut rules = Vec::new();
        if let Some(rules_list) = compound.get("rules").and_then(|t| t.list())
            && let Some(compounds) = rules_list.compounds()
        {
            for rule_compound in compounds {
                let mut blocks = Vec::new();
                if let Some(blocks_list) = rule_compound.get("blocks").and_then(|t| t.list())
                    && let Some(strings) = blocks_list.strings()
                {
                    for s in strings {
                        if let Ok(id) = s.to_str().parse() {
                            blocks.push(id);
                        }
                    }
                }

                let speed = rule_compound.get("speed").and_then(|t| t.float());
                let correct_for_drops = rule_compound
                    .get("correct_for_drops")
                    .and_then(|t| t.byte())
                    .map(|b| b != 0);

                rules.push(ToolRule {
                    blocks,
                    speed,
                    correct_for_drops,
                });
            }
        }

        let default_mining_speed = compound
            .get("default_mining_speed")
            .and_then(|t| t.float())
            .unwrap_or(1.0);

        let damage_per_block = compound
            .get("damage_per_block")
            .and_then(|t| t.int())
            .unwrap_or(1);

        let can_destroy_blocks_in_creative = compound
            .get("can_destroy_blocks_in_creative")
            .and_then(|t| t.byte())
            .map(|b| b != 0)
            .unwrap_or(true);

        Some(Self {
            rules,
            default_mining_speed,
            damage_per_block,
            can_destroy_blocks_in_creative,
        })
    }
}
