//! Villager trade table types

pub struct VillagerTrade {
    pub wants: &'static str,
    pub wants_count: i32,
    pub additional: Option<(&'static str, i32)>,
    pub gives: &'static str,
    pub gives_count: i32,
    pub max_uses: i32,
    pub xp: i32,
}

pub struct VillagerTradeLevel {
    pub amount: i32,
    pub trades: &'static [VillagerTrade],
}

pub struct ProfessionTradeTable {
    pub profession: &'static str,
    pub levels: &'static [VillagerTradeLevel],
}
