use rustc_hash::FxHashMap;
use steel_registry::data_components::vanilla_components::USE_COOLDOWN;
use steel_registry::item_stack::ItemStack;
use steel_utils::Identifier;

#[derive(Clone, Copy)]
struct CooldownInstance {
    end_time: i32,
}

#[derive(Default)]
pub(super) struct ItemCooldowns {
    cooldowns: FxHashMap<Identifier, CooldownInstance>,
    tick_count: i32,
}

impl ItemCooldowns {
    #[must_use]
    pub(super) fn is_on_cooldown(&self, stack: &ItemStack) -> bool {
        let group = cooldown_group(stack);
        self.cooldowns
            .get(&group)
            .is_some_and(|cooldown| cooldown.end_time > self.tick_count)
    }

    pub(super) fn tick(&mut self) -> Vec<Identifier> {
        self.tick_count = self.tick_count.wrapping_add(1);
        let mut ended = Vec::new();
        self.cooldowns.retain(|group, cooldown| {
            if cooldown.end_time <= self.tick_count {
                ended.push(group.clone());
                false
            } else {
                true
            }
        });
        ended
    }

    pub(super) fn add_from_stack(&mut self, stack: &ItemStack) -> Option<(Identifier, i32)> {
        let cooldown = stack.get(USE_COOLDOWN)?;
        let duration = cooldown.ticks();
        if duration <= 0 {
            return None;
        }
        let group = cooldown_group(stack);
        self.cooldowns.insert(
            group.clone(),
            CooldownInstance {
                end_time: self.tick_count + duration,
            },
        );
        Some((group, duration))
    }
}

fn cooldown_group(stack: &ItemStack) -> Identifier {
    stack
        .get(USE_COOLDOWN)
        .and_then(|cooldown| cooldown.cooldown_group.clone())
        .unwrap_or_else(|| stack.item().key.clone())
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::vanilla_components::{USE_COOLDOWN, UseCooldown};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{test_support::init_test_registry, vanilla_items};

    use super::ItemCooldowns;

    #[test]
    fn cooldown_blocks_until_duration_ticks_pass() {
        init_test_registry();

        let stack = ItemStack::with_count(&vanilla_items::ITEMS.ender_pearl, 1);
        let mut cooldowns = ItemCooldowns::default();

        let Some((group, duration)) = cooldowns.add_from_stack(&stack) else {
            panic!("ender pearl should have a use cooldown");
        };

        assert_eq!(group, vanilla_items::ITEMS.ender_pearl.key);
        assert_eq!(duration, 20);
        assert!(cooldowns.is_on_cooldown(&stack));

        for _ in 0..19 {
            assert!(cooldowns.tick().is_empty());
            assert!(cooldowns.is_on_cooldown(&stack));
        }

        assert_eq!(
            cooldowns.tick(),
            vec![vanilla_items::ITEMS.ender_pearl.key.clone()]
        );
        assert!(!cooldowns.is_on_cooldown(&stack));
    }

    #[test]
    fn explicit_group_is_shared_between_items() {
        init_test_registry();

        let group = steel_utils::Identifier::vanilla_static("test_group");
        let mut stack = ItemStack::with_count(&vanilla_items::ITEMS.ender_pearl, 1);
        stack.set(USE_COOLDOWN, UseCooldown::new(0.5, Some(group.clone())));
        let mut other = ItemStack::with_count(&vanilla_items::ITEMS.chorus_fruit, 1);
        other.set(USE_COOLDOWN, UseCooldown::new(1.0, Some(group.clone())));
        let unrelated = ItemStack::with_count(&vanilla_items::ITEMS.wind_charge, 1);
        let mut cooldowns = ItemCooldowns::default();

        let Some((started_group, duration)) = cooldowns.add_from_stack(&stack) else {
            panic!("stack should have a use cooldown");
        };

        assert_eq!(started_group, group);
        assert_eq!(duration, 10);
        assert!(cooldowns.is_on_cooldown(&stack));
        assert!(cooldowns.is_on_cooldown(&other));
        assert!(!cooldowns.is_on_cooldown(&unrelated));
    }
}
