
use std::fs;

use proc_macro2::TokenStream;
use quote::quote;
use serde::Deserialize;

const TRADE_SET_DIR: &str = "../steel-utils/build_assets/builtin_datapacks/minecraft/trade_set";
const TAG_DIR: &str = "../steel-utils/build_assets/builtin_datapacks/minecraft/tags/villager_trade";
const TRADE_DIR: &str = "../steel-utils/build_assets/builtin_datapacks/minecraft/villager_trade";
const MAX_LEVEL: usize = 5;

#[derive(Deserialize)]
struct TradeSet {
    amount: f64,
    trades: String,
}

#[derive(Deserialize)]
struct TagFile {
    values: Vec<String>,
}

struct SimpleTrade {
    wants: String,
    wants_count: i32,
    additional: Option<(String, i32)>,
    gives: String,
    gives_count: i32,
    max_uses: i32,
    xp: i32,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed={TRADE_SET_DIR}");
    println!("cargo:rerun-if-changed={TAG_DIR}");
    println!("cargo:rerun-if-changed={TRADE_DIR}");

    let mut professions: Vec<String> = fs::read_dir(TRADE_SET_DIR)
        .expect("trade_set dir should exist")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    professions.sort();

    let mut profession_tokens = TokenStream::new();
    for profession in &professions {
        let mut level_tokens = TokenStream::new();
        for level in 1..=MAX_LEVEL {
            let Some((amount, trades)) = read_level(profession, level) else {
                continue;
            };
            let mut trade_tokens = TokenStream::new();
            for trade in trades {
                let SimpleTrade { wants, wants_count, additional, gives, gives_count, max_uses, xp } = trade;

                let additional = match additional {
                    Some((item, count)) => quote! { Some((#item, #count)) },
                    None => quote! { None }
                };
                trade_tokens.extend(quote! {
                    VillagerTrade {
                        wants: #wants,
                        wants_count: #wants_count,
                        additional: #additional,
                        gives: #gives,
                        gives_count: #gives_count,
                        max_uses: #max_uses,
                        xp: #xp,
                    },
                });
            }
            level_tokens.extend(quote! {
                ProfessionTradeTable { amount: #amount, trades: &[#trade_tokens] },
            });
        }
        profession_tokens.extend(quote! {
            ProfessionTradeTable { profession: #profession, levels: &[#level_tokens] },
        });
    }

    quote! {
        use crate::villager_trade::{ProfessionTradeTable, VillagerTrade, VillagerTradeLevel};

        pub static VILLAGER_TRADES: &[ProfessionTradeTable] = &[
            #profession_tokens
        ];
    }
}

fn read_level(profession: &str, level: usize) -> Option<(i32, Vec<SimpleTrade>)> {
    let trade_set: TradeSet = read_json(&format!("{TRADE_SET_DIR}/{profession}/level_{level}.json"))?;
    let tag_ref = trade_set.trades.trim_start_matches('#');
    let tag_path = tag_ref.strip_prefix("minecraft").unwrap_or(tag_ref);
    let tag: TagFile = read_json(&format!("{TAG_DIR}/{tag_path}.json"))?;

    let trades = tag
        .values
        .iter()
        .filter_map(|id| {
            let path = id.strip_prefix("minecraft:").unwrap_or(id);
            let value: serde_json::Value = read_json(&format!("{TRADE_DIR}/{path}.json"))?;
            parse_simple_trade(&value)
        })
        .collect();

    Some((trade_set.amount as i32, trades))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Option<T> {
    let content = fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&content).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}")))
}

fn parse_simple_trade(value: &serde_json::Value) -> Option<SimpleTrade> {
    const ALLOWED: &[&str] = &[
        "wants",
        "additional_wants",
        "gives",
        "max_uses",
        "reputation_discount",
        "xp",
    ];
    let obj = value.as_object()?;
    if obj.keys().any(|key| !ALLOWED.contains(&key.as_str())) {
        return None;
    }

    let (wants, wants_count) = parse_item(obj.get("wants")?)?;
    let additional = match obj.get("additional_wants") {
        Some(item) => Some(parse_item(item)?),
        None => None,
    };
    let (gives, gives_count) = parse_item(obj.get("gives")?)?;
    let max_uses = obj.get("max_uses").and_then(serde_json::Value::as_f64).map_or(4, |v| v as i32);
    let xp = obj.get("xp").and_then(serde_json::Value::as_f64).map_or(1, |v| v as i32);

    Some(SimpleTrade { wants, wants_count, additional, gives, gives_count, max_uses, xp })
}

fn parse_item(value: &serde_json::Value) -> Option<(String, i32)> {
    let obj = value.as_object()?;
    if obj.keys().any(|key| key != "id" && key != "count") {
        return None;
    }
    let id = obj.get("id")?.as_str()?.to_owned();
    let count = obj.get("count").and_then(serde_json::Value::as_f64).map_or(1, |v| v as i32);
    Some((id, count))
}
