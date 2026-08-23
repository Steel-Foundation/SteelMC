use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Debug, Deserialize)]
struct Item {
    id: String,
    #[serde(default = "one")]
    count: f64,
}

#[derive(Debug, Deserialize)]
struct Trade {
    gives: Item,
    wants: Item,
    #[serde(default)]
    additional_wants: Option<Item>,
    max_uses: f64,
    #[serde(default)]
    xp: f64,
    #[serde(default)]
    reputation_discount: f64,
    #[serde(default)]
    given_item_modifiers: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct TradeSet {
    amount: f64,
}

const fn one() -> f64 {
    1.0
}

pub fn build(manifest_dir: &str) -> String {
    let root = Path::new(manifest_dir).join("../generated/data/minecraft/villager_trade");
    let mut rows = Vec::new();
    let entries = fs::read_dir(&root).unwrap_or_else(|error| {
        panic!(
            "failed to read villager trade directory {}: {error}",
            root.display()
        )
    });
    for profession in entries {
        let profession = profession.expect("failed to read villager profession directory");
        let profession_path = profession.path();
        if !profession_path.is_dir() {
            continue;
        }
        let profession_name = profession.file_name().to_string_lossy().into_owned();
        for tier in fs::read_dir(&profession_path).expect("failed to read villager tier directory")
        {
            let tier = tier.expect("failed to read villager trade tier");
            let tier_path = tier.path();
            if !tier_path.is_dir() {
                continue;
            }
            let tier_number: u8 = tier
                .file_name()
                .to_string_lossy()
                .parse()
                .unwrap_or_else(|_| panic!("invalid villager trade tier {}", tier_path.display()));
            for file in fs::read_dir(&tier_path).expect("failed to read villager trade files") {
                let file = file.expect("failed to read villager trade file");
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let raw = fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
                let trade: Trade = serde_json::from_str(&raw).unwrap_or_else(|error| {
                    panic!("invalid villager trade {}: {error}", path.display())
                });
                let trade_name = path.file_stem().expect("trade filename").to_string_lossy();
                rows.push((
                    profession_name.clone(),
                    tier_number,
                    format!("minecraft:{profession_name}/{tier_number}/{trade_name}"),
                    trade,
                ));
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let tags_root = Path::new(manifest_dir).join("../generated/data/minecraft/tags/villager_trade");
    let mut tags = BTreeMap::<String, Vec<String>>::new();
    for file in walk_json(&tags_root) {
        let rel = file.strip_prefix(&tags_root).expect("tag path under root");
        let tag_name = rel.with_extension("").to_string_lossy().replace('/', "/");
        #[derive(Deserialize)]
        struct Tag {
            values: Vec<String>,
        }
        let tag: Tag = serde_json::from_str(&fs::read_to_string(&file).expect("read trade tag"))
            .expect("parse trade tag");
        tags.insert(tag_name, tag.values);
    }

    let trade_sets_root = Path::new(manifest_dir).join("../generated/data/minecraft/trade_set");
    let mut amounts = BTreeMap::<(String, u8), u8>::new();
    for file in walk_json(&trade_sets_root) {
        let relative = file
            .strip_prefix(&trade_sets_root)
            .expect("trade set path under root");
        let Some(profession) = relative
            .parent()
            .and_then(|path| path.to_str())
            .filter(|profession| !profession.contains('/'))
        else {
            continue;
        };
        let Some(level) = relative.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some(level) = level.strip_prefix("level_") else {
            continue;
        };
        let tier = level
            .parse::<u8>()
            .unwrap_or_else(|_| panic!("invalid trade set level in {}", file.display()));
        let set: TradeSet = serde_json::from_str(
            &fs::read_to_string(&file)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", file.display())),
        )
        .unwrap_or_else(|error| panic!("invalid trade set {}: {error}", file.display()));
        let amount = integer(set.amount);
        let amount = u8::try_from(amount)
            .unwrap_or_else(|_| panic!("trade set amount exceeds u8 in {}", file.display()));
        amounts.insert((profession.to_owned(), tier), amount);
    }

    let mut out = String::from(
        "#[derive(Debug, Clone, Copy)]\npub struct TradeItem { pub id: &'static str, pub count: u32 }\n#[derive(Debug, Clone, Copy)]\npub struct TradeData { pub wants: TradeItem, pub additional_wants: Option<TradeItem>, pub gives: TradeItem, pub max_uses: u32, pub xp: u32, pub reputation_discount: f32, pub has_item_modifiers: bool }\n#[derive(Debug, Clone, Copy)]\npub struct ProfessionTrades { pub profession: &'static str, pub tier: u8, pub amount: u8, pub trades: &'static [TradeData] }\n",
    );
    for (index, (_, _, _, _)) in rows.iter().enumerate() {
        let (_, _, _, trade) = &rows[index];
        out.push_str(&format!("const TRADE_{index}: TradeData = TradeData {{ wants: TradeItem {{ id: {:?}, count: {} }}, additional_wants: {}, gives: TradeItem {{ id: {:?}, count: {} }}, max_uses: {}, xp: {}, reputation_discount: {:?}, has_item_modifiers: {} }};\n",
            trade.wants.id, integer(trade.wants.count), item_opt(&trade.additional_wants), trade.gives.id, integer(trade.gives.count), integer(trade.max_uses), integer(trade.xp), trade.reputation_discount as f32, trade.given_item_modifiers.is_some()));
    }
    out.push_str("pub static VILLAGER_TRADES: &[ProfessionTrades] = &[\n");
    let index_by_key = rows
        .iter()
        .enumerate()
        .map(|(i, (_, _, key, _))| (key.clone(), i))
        .collect::<BTreeMap<_, _>>();
    let mut emitted = BTreeSet::new();
    for (profession, tier, _, _) in &rows {
        if !emitted.insert((profession.clone(), *tier)) {
            continue;
        }
        let tag_key = format!("{profession}/level_{tier}");
        let Some(values) = tags.get(&tag_key) else {
            continue;
        };
        let amount = amounts
            .get(&(profession.clone(), *tier))
            .copied()
            .unwrap_or_else(|| panic!("missing trade set for {profession}/level_{tier}"));
        let mut resolved = BTreeSet::new();
        resolve_values(values, &tags, &mut resolved);
        let refs = resolved
            .into_iter()
            .filter_map(|value| index_by_key.get(&value).copied())
            .map(|i| format!("TRADE_{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        if refs.is_empty() {
            continue;
        }
        out.push_str(&format!("    ProfessionTrades {{ profession: {:?}, tier: {tier}, amount: {amount}, trades: &[{}] }},\n", profession, refs));
    }
    out.push_str("];\n");
    out
}

fn walk_json(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk_json(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files
}

fn resolve_values(
    values: &[String],
    tags: &BTreeMap<String, Vec<String>>,
    out: &mut BTreeSet<String>,
) {
    for value in values {
        if let Some(tag) = value.strip_prefix("#minecraft:") {
            if let Some(nested) = tags.get(tag) {
                resolve_values(nested, tags, out);
            }
        } else {
            out.insert(value.clone());
        }
    }
}

fn integer(value: f64) -> u32 {
    assert!(
        value.is_finite() && value >= 0.0 && value.fract() == 0.0,
        "trade numeric field must be a non-negative integer: {value}"
    );
    value as u32
}

fn item_opt(item: &Option<Item>) -> String {
    item.as_ref()
        .map(|item| {
            format!(
                "Some(TradeItem {{ id: {:?}, count: {} }})",
                item.id,
                integer(item.count)
            )
        })
        .unwrap_or_else(|| "None".to_owned())
}
