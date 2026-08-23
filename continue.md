# Continuation Plan: Fish Movement + Villager Trading Completion

Status snapshot after deep source research. Both root-cause analyses are DONE; implementation is IN PROGRESS. Do not re-litigate the diagnoses below — they are verified against SteelMC source AND vanilla 26.2 source (`minecraft-src/`).

---

## PART A — FISH MOVEMENT (implementation started)

### Verified root causes (in execution order per tick)

1. **Ground MoveControl runs for fish** (`steel-core/src/entity/mob/mod.rs:1394` `tick_move_control`, default trait method in `pub trait Mob` at line 332–1617).
   - `tick_move_to_control` (line 1429) calls `should_jump_to_wanted_position` (line 1627) when `yd > max_up_step && xd²+zd² < max(width,1)` → sets jump flag.
   - `handle_living_jump` (`steel-core/src/entity/living_entity.rs:2073`) → deep-water branch line 2100: `jump_in_liquid(WATER)` = **+0.04 Y velocity EVERY tick** while flag set. THIS is what drives fish to the surface and makes them bob there (target above surface ⇒ yd never shrinks ⇒ jumps forever).
   - Vanilla fish never hit this because `AbstractFish` constructor replaces moveControl with `FishMoveControl` (no jump branch).

2. **No vertical steering / no buoyancy**: vanilla `AbstractFish.FishMoveControl.tick()` (26.2 source confirmed):
   - if eye in water: `vel.y += 0.005`
   - if op==MOVE_TO && !nav.isDone(): `setSpeed(lerp(0.125, speed, speedModifier*movementSpeed))`; if yd != 0: `vel.y += speed * (yd/dd) * 0.1`; yaw rotlerp 90°, bodyRot=yaw; else `setSpeed(0)`.
   - SteelMC has none of this (previous AI's input.y hack in Cod::travel never engaged because `tick_move_control` already consumed MoveTo→Wait BEFORE travel reads it).

3. **Navigation paths hug the surface**:
   - `WalkNodeEvaluator::get_start` (node_evaluator.rs:121–127): `can_float && in_water` climbs start node to TOP of water column.
   - `get_neighbors` (line 157+) generates ONLY horizontal neighbors (+diagonals) — no vertical movement possible in paths.
   - `tick_path_navigation_target` (`entity/mob/pathfinder.rs:19–53`): wanted-Y = floor-snap via `getGroundY` ground logic; `ground_navigation_surface_y` (pathfinder.rs:68) snaps nav temp-pos to water surface when can_float.
   - Vanilla fish use `WaterBoundPathNavigation` + `SwimNodeEvaluator`: 6-direction neighbors, `getGroundY`=identity, `getTempMobPos`=bbox mid-height, `canUpdatePath`=isInLiquid.

4. **Fish travel itself is correct-ish**: Cod::travel already mirrors vanilla `AbstractFish.travelInWater` (moveRelative(0.01), move, scale 0.9, target==null → −0.005). Keep semantics, remove debug spam/hack.

### Implementation plan (EXACT steps, partially done)

**Step 1 — walk/mod.rs (DONE, just written)**: Added `pub trait NodeEvaluator { reset_search_state, node, node_mut, nodes_mut, get_start, get_neighbors }`. `get_neighbors` uses `&mut dyn WalkNodeCollision` (APIT not allowed in trait methods). Exports SwimNodeEvaluator.

**Step 2 — node_evaluator.rs (NEXT EDIT, was interrupted here)**:
- Bump `WalkNeighbors.nodes: [Option<i32>; 8]` → `[Option<i32>; 10]` (swim = 6 cardinal + 4 diagonal). Located ~line 32–35 (`pub struct WalkNeighbors`). My earlier edit failed due to whitespace mismatch — read exact lines first (rtk output showed `nodes: [None; 8],` inside `new()` around line 31).
- Change inherent `get_neighbors(... collision: &mut impl WalkNodeCollision ...)` → `&mut dyn WalkNodeCollision`.
- Add `impl NodeEvaluator for WalkNodeEvaluator` delegating to existing methods.
- Check `walk/tests.rs` (22KB) for direct callers needing signature updates.

**Step 3 — NEW FILE `walk/swim_node_evaluator.rs`**: Port vanilla `SwimNodeEvaluator.java` (read it fully; ~130 lines):
```rust
pub struct SwimNodeEvaluator {
    settings: MobPathSettings,   // reuse wholesale: dims, malus, bounding_box accessors all exist
    allow_breaching: bool,
    nodes: NodeStore,
    path_types_by_pos_cache: rustc_hash::FxHashMap<(i32,i32,i32), PathType>,
}
```
- `get_start`: `nodes.get_node(floor(bb.min_x()), floor(bb.min_y()+0.5), floor(bb.min_z())).hash()` (settings.bounding_box() accessor exists).
- `get_neighbors`: 6 directions (steel_utils Direction::Down/Up/North/South/West/East with `.offset() -> (i32,i32,i32)`); valid = node exists && !closed; then horizontal diagonals where both adjacent cardinal nodes have cost_malus >= 0.0 (vanilla hasMalus).
- `find_accepted_node`: type = cached_block_type; accept if `(allow_breaching && type==Breach) || type==Water`; malus check `settings.pathfinding_malus(type) >= 0.0`; set node.path_type, `cost_malus = max(existing, path_cost)`; if block fluid empty → `+= 8.0`; return hash or None.
- `get_path_type_of_mob(context,x,y,z)`: triple loop x..x+width, y..y+height, z..z+depth over entity volume: if fluid empty && below.is_pathfindable(PathComputationType::Water) && block.is_air() → Breach; if !fluid.is_water() → Blocked; finally `is_pathfindable(Water)` → Water else Blocked.
- Cache path types per pos (manual entry check to avoid closure borrow issues).
- impl NodeEvaluator (collision param ignored — vanilla swim uses no collision).
- Watch out: `node.cost_malus.max(path_cost)` pattern from `get_node_and_update_cost_to_max` (node_evaluator.rs:698).

**Step 4 — `ai/pathfinder.rs` (ai/pathfinder.rs = PathFinder struct)**:
- `find_path`, `prepare_search`, `best_reached_path`, `best_unreached_path`: change `&mut/& WalkNodeEvaluator` params → generic `<E: NodeEvaluator>` (or `&mut impl NodeEvaluator` in inherent fns — APIT fine in inherent).
- Line 98 `evaluator.get_neighbors(context, collision, current_hash)` — collision reborrow as dyn.

**Step 5 — navigation.rs + mob/pathfinder.rs**:
- `PathNavigation`: add private `water_bound: bool` (default false) + getter/setter (pattern copy of can_float at lines 66/173–178; test at 837 mirrors).
- `create_path(evaluator: &mut impl NodeEvaluator, ...)`.
- In `PathfinderMob::create_path_to_targets` (`mob/pathfinder.rs:288`): pick evaluator by `navigation.water_bound()`:
  - water → `SwimNodeEvaluator::new(MobPathSettings::from_mob(self), false)` (allow_breaching only for dolphin)
  - else → existing `WalkNodeEvaluator::new(settings)`
  - Use a small local enum wrapper implementing NodeEvaluator OR duplicate the create_path call per branch.
- `tick_path_navigation_target` (lines 19–53):
  - temp mob pos: if `navigation.water_bound()` → `DVec3::new(pos.x, (bb.min_y()+bb.max_y())/2.0, pos.z)` (vanilla WaterBound.getTempMobPos = bbox mid-Y); else existing `ground_navigation_temp_mob_pos`.
  - wanted Y (lines 46–52): water_bound → use `target.y` unchanged (vanilla WaterBound.getGroundY identity); else keep floor/air-below logic.
- `PathfinderMob::can_update_path` (pathfinder.rs:117): water_bound → `self.is_in_water() || self.is_in_lava()`; else existing.

**Step 6 — NEW FILE `entity/mob/fish.rs`** (register `mod fish; pub use fish::...` in mob/mod.rs):
- Constants: TRAVEL_SPEED 0.01f32, DRAG 0.9f64, SINK −0.005f64, BUOYANCY +0.005f64.
- `pub fn tick_move_control<M: Mob + ?Sized>(mob: &M)`: FishMoveControl port per spec above (snapshot+consume MoveTo→Wait under one controls lock like mod.rs:1395–1402; separate `mob.mob_base().navigation().lock().is_done()` check; lerp inline `speed + (target−speed)*0.125` f32; yaw via `rotlerp` — find its definition in mob/mod.rs (free fn, search "fn rotlerp"); set_y_body_rot(yaw)).
- `pub fn travel<M: Mob + ?Sized>(mob: &M, input: DVec3) -> Option<MoveResult>`: travelInWater port (move_relative(0.01,input); move_entity(SelfMovement, vel)?; vel*=0.9; target().is_none() → vel.y −= 0.005).
- `pub fn init_mob_base(mob_base: &MobBase)`: nav.set_water_bound(true) + `mob_base.pathfinding_malus().lock().set(PathType::Water, 0.0)` (vanilla WaterAnimal ctor). NO can_float (remove from cod!).
- Temporary debug (user-requested): env-gated `STEEL_FISH_DEBUG` (OnceLock<bool>), log stages for `mob.id() % 100 == 0`: vel before/after move control, op/wanted pos, input, vel before/after move_entity, final vel, pos, is_in_water, on_ground, nav done. Marked temporary.

**Step 7 — fish entities** (`entities/mobs/passive/{cod,salmon,tropical_fish,pufferfish}/mod.rs`):
- Cod: replace goal registrations with vanilla parity: Panic(0, 1.25), AvoidEntity(2, 8.0, 1.6, 1.4) players-only?, RandomSwimming(4, 1.0, 40). REMOVE LookAtPlayerGoal/RandomLookAroundGoal (not vanilla), FloatGoal import unused, remove `nav.set_can_float(true)` → call fish::init_mob_base(&mob_base), DELETE entire travel override incl. debug spam (add `fn tick_move_control` override → fish::tick_move_control(self)), replace ai_step flop logic? NO — keep flop (matches vanilla AbstractFish.aiStep), remove travel override entirely so LivingEntity::default_travel routes to... WAIT: must override `travel_in_water` instead IF it's an overridable trait method (living_entity.rs:2452 sits in `pub trait LivingEntity` starting line 12 → yes default method). Prefer overriding `travel_in_water(self, input, base_gravity, is_falling, old_y)` → `fish::travel(self, input)` ignoring gravity args exactly like vanilla AbstractFish. Then delete Cod's whole custom travel override.
- Salmon/TropicalFish/Pufferfish: currently land-style stubs (FloatGoal(0)+WaterAvoidingRandomStroll(5)+LookAt(6)+RandomLookAround(7)). Convert to same fish foundation (goals per vanilla AbstractFish; pufferfish KEEP its existing puffer-specific goals/logic if any beyond the stub — inspect file first; don't fake missing puff mechanics, leave TODO).
- Verify `set_pathfinding_malus` availability: `MobBase.pathfinding_malus()` SyncMutex<PathfindingMalus> with `.set(path_type, malus)` (path.rs:336). Mob trait may need a passthrough — check for existing `set_pathfinding_malus` on trait (only `get_pathfinding_malus` found at mod.rs:1096); add tiny default method if absent.

### Tests for fish
- Unit: swim evaluator get_path_type_of_mob (water→Water, air-over-water→Breach, solid→Blocked), find_accepted_node accepts Water w/ malus≥0 & adds +8 for non-fluid block, neighbors include Up/Down.
- PathFinder finds a path through water column downward/upward (mini world harness — see walk/tests.rs patterns).
- Existing walk tests stay green (signature updates only).

---

## PART B — VILLAGER WANT-SIDE COMPONENTS (plan finalized, not started)

Only ONE extracted trade has components: `generated/data/minecraft/villager_trade/wandering_trader/water_bottle_emerald.json` → wants.components `{"minecraft:potion_contents":{"potion":"minecraft:water"}}`.

1. **build/trades.rs**: add `components: Option<serde_json::Value>` to `Item` struct; generate TradeItem field `components_json: Option<&'static str>` (compact JSON string via serde_json::to_string). Regenerates steel-core/src/entity/generated? NO — output is OUT_DIR/villager_trades.rs (build-time, fine).
2. **steel-registry** small API additions:
   - core.rs: make `DataComponentExactPredicate::from_owned_nbt` (line 331, currently private `fn`) → `pub fn`.
   - Add `pub fn matches_stack(&self, stack: &ItemStack) -> bool`: `self.values.iter().all(|(entry,value)| stack.get_effective_value_raw(&entry.key) == Some(value))` (ComponentData: PartialEq ✓, ItemStack::get_effective_value_raw ✓ item_stack.rs:406).
3. **steel-core villager/mod.rs**:
   - New `CostItem { stack: ItemStack, components: DataComponentExactPredicate }`; MerchantOffer fields become cost_a: CostItem, cost_b: Option<CostItem>.
   - `resolve_components(json: &str) -> Option<DataComponentExactPredicate>`: parse serde_json → convert to simdnbt owned NbtTag (Object→Compound, Array→NbtList, String, i64-in-i32→Int else Double, bool→Byte) → for each key: REGISTRY.data_components.by_key → entry.read_nbt_owned(tag) → collect → DataComponentExactPredicate::new(values)?.
   - from_data resolves predicates; can_trade adds `components.matches_stack(b/a)` checks (vanilla ItemCost.test = item match && predicate.test).
4. **merchant_menu.rs to_packet**: protocol `ItemCost { item, count, components }` built from CostItem (wire now carries predicates → client UI shows correct requirement).
5. Tests: water-bottle trade resolves non-empty predicate; matches water potion_contents stack, rejects plain potion / different potion; round-trip through MerchantOfferPacket encoding includes predicate bytes.

---

## PART C — VALIDATION GATES (after both parts)

```
cargo check --workspace
cargo test -p steel-core
cargo test -p steel-protocol
cargo clippy --workspace --all-targets --all-features   # pre-existing warnings in entity mob files are NOT mine
cargo fmt --all -- --check                              # NOTE: repo has PRE-EXISTING fmt diffs in cod/mod.rs, iron_golem/mod.rs etc. — do NOT cargo fmt --all; only format touched files via rustfmt <files>
```

Real-client acceptance (USER must run MC 26.2 client; cannot be done in this environment):
- Villager: right-click farmer → UI opens, 2 offers visible, select, insert wheat/emerald per offer dump in stderr, result appears, trade completes once, inputs consumed, uses increments, reopen works.
- Fish: spawn cod deep in water → stays submerged & swims 3D; targets above/below/horizontal honored; no surface sticking; land flop preserved. Debug via `STEEL_FISH_DEBUG=1 cargo run`.

## Report format owed to user
A. Protocol correctness PASS/FAIL/NOT TESTED · B. Server-side logic correctness · C. Real client verification — plus the 13 numbered answers from their prompt (fish root cause, runtime stage, architectural fix location = GENERIC movement/pathfinding layer not cod-specific, villager status, files changed, tests run, limitations: e.g., allow_breaching=false until dolphin support, WalkNodeEvaluator still used for land mobs, salmon/tropical/pufferfish conversion depth).

## Key file inventory touched so far
- `steel-core/src/entity/ai/walk/mod.rs` — REWRITTEN with NodeEvaluator trait (done).
- Everything else pending per steps above.
- Villager protocol fix from previous session is DONE and tested — do not touch `steel-protocol/src/packets/game/merchant.rs` logic (only extend if predicate wiring requires).
