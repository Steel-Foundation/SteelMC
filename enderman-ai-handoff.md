# Enderman AI — Handoff for the next AI session

**Repo:** `/home/thanawat/steelmc-b` (SteelMC, Rust Minecraft server, nightly toolchain — `export PATH="$HOME/.cargo/bin:$PATH"`)
**Task:** Fix enderman AI — it could stay in water indefinitely and did nothing when a player stared at its eyes.
**Status:** Core + most vanilla gaps implemented. `cargo test -p steel-core enderman` passes, clippy clean (only the pre-existing `absolute_paths` warning on the untouched `finalize_spawn` signature). **Changes are NOT committed.**

> The working tree is dirty with a lot of unrelated uncommitted work. Separate this task's files when committing.
> Pre-existing unrelated test failure: `chunk_saver::storage::tests::entities::unimplemented_block_entities_preserve_nbt_through_proto_save_load`.

---

## 1. Files changed by this task

| File | What changed |
|---|---|
| `steel-core/src/entity/entities/mobs/hostile/enderman.rs` | Main implementation (all AI/teleport/target logic + freeze goal + tests) |
| `steel-core/src/entity/mod.rs` | `is_in_rain()` made `pub(crate)` (1 line — NOTE: this file also has **unrelated uncommitted** changes: `tamable` module, arrow spawn exports) |
| `steel-core/src/world/player_spawn_finder.rs` | `aabb_contains_any_liquid()` made `pub(crate)` so the enderman reuses it |

---

## 2. Implemented (and the vanilla source it mirrors)

Vanilla source: `minecraft-src/minecraft/src/net/minecraft/world/entity/monster/EnderMan.java`.

### 2.1 Removed generic "attack players on sight"
The old `NearestAttackableTargetGoal::new_for_players(true, |_, _| true)` is gone. Vanilla endermen only target players via stare/anger (`registerGoals` ~102) plus `HurtByTargetGoal` (~103).

### 2.2 Stare → anger
`custom_server_ai_step` scans nearby players (FOLLOW_RANGE = 64) for one that is a valid target **and** is looking at the enderman's eyes via `is_being_stared_by` (a module-level fn shared with the freeze goal):
- exact vanilla `LivingEntity.isLookingAtMe(player, 0.025, true, false, eyeY)` math,
- carved-pumpkin rejection via the `GAZE_DISGUISE_EQUIPMENT` item tag (verified: contains **only** `minecraft:carved_pumpkin` — do NOT add jack_o_lantern).

On detection: `creepy` + `stared_at` synced flags, `ENTITY_ENDERMAN_SCREAM`, target set so `MeleeAttackGoal` chases. When nobody stares, `set_target(None)` runs the vanilla reset path.

### 2.3 Water / rain drown damage
`is_in_water() || is_in_rain()` → `hurt(DROWN, 1.0)`. Vanilla mirrors `isSensitiveToWater()` + `LivingEntity` drown tick. Steel has no `is_sensitive_to_water` hook, so it lives in `custom_server_ai_step`; migrate if Steel ever adds one.

### 2.4 Teleport away on damage
`Entity::hurt` mirrors `EnderMan.hurtServer`:
- **projectile** (`IS_PROJECTILE` tag — verified to include `arrow`, `trident`, `mob_projectile`, `fireball`, `wither_skull`, `thrown`, `wind_charge`): up to 64 `teleport()` attempts, **no damage taken** (vanilla returns `true`).
- **non-projectile from a non-living source** (e.g. drown): damage applies, then teleport 9/10.

Teleport helpers: `teleport()`, `teleport_to()`, `random_teleport()`, `has_teleport_landing_space()` (reuses the spawn-finder liquid helper).

### 2.5 Teleport FX
On success: `GameEvent.TELEPORT` at the old position + `ENTITY_ENDERMAN_TELEPORT` played once at the old position and once at the new one (vanilla `EnderMan.teleport` ~291-295). Portal particles are client-side in vanilla → skipped.

### 2.6 `setTarget` side effects
`Mob::set_target` override mirrors `EnderMan.setTarget` (~122-138): target set → `targetChangeTime = tickCount`, `creepy = true`, transient `attacking` movement-speed modifier (+0.15, `AddValue`); target cleared → `targetChangeTime = 0`, `creepy`/`stared_at` false, modifier removed. `add_modifier` already no-ops when present, so no extra `hasModifier` guard.

### 2.7 Daylight flee
`custom_server_ai_step` mirrors ~244-251: `is_bright_outside() && tickCount >= targetChangeTime + 600`, `light_level_dependent_magic_value > 0.5`, `can_see_sky`, `random * 30 < (br - 0.4) * 2` → clear target + teleport.

### 2.8 `EndermanFreezeWhenLookedAt` goal
Private `EndermanFreezeWhenLookedAtGoal` in `enderman.rs` at priority 1, controls `JUMP | MOVE`. Claiming `MOVE` is what actually freezes it — the goal selector stops `MeleeAttackGoal` (priority 2, `MOVE | LOOK`) while this runs. `can_use` doubles as `can_continue_to_use` (vanilla `Goal` default). Runs every tick via `requires_update_every_tick`. Stops navigation on start, faces the player via `LookControl::set_look_at` with vanilla's `Mob.getHeadRotSpeed()` / `getMaxHeadXRot()` (10 / 40).
`ENTITY_ENDERMAN_STARE` is `playLocalSound` from the client (`playStareSound` / `onSyncedDataUpdated`) → no server-side work.

### 2.9 Endermite targeting
Target selector priority 2: `NearestAttackableTargetGoal::new(true, |target, _| target.entity_type() == &vanilla_entities::ENDERMITE)` (vanilla ~104).

### 2.10 Ambient sound
`ENTITY_ENDERMAN_SCREAM` when `creepy`, else `ENTITY_ENDERMAN_AMBIENT` (vanilla `getAmbientSound` ~304).

---

## 3. Verification

- `cargo check -p steel-core` → clean.
- `cargo clippy -p steel-core --all-targets` → no new warnings in `enderman.rs`.
- `cargo fmt -p steel-core -- --check` → `enderman.rs` clean.
- `cargo test -p steel-core enderman` → 2 passed:
  - `enderman_target_grants_the_vanilla_attacking_speed_boost`
  - `enderman_freeze_goal_claims_move_and_jump`
- Runtime: `./deploytest.sh`, then summon an enderman and (1) stare at its eyes → scream + jaw opens + freezes, attacks when you look away; wearing a carved pumpkin prevents it; (2) drop it in water → damage + teleport away; (3) shoot an arrow → it dodges and takes no damage.

---

## 4. Not implemented yet (verify against the cited vanilla source before coding)

1. **5-tick aggro delay** in `EndermanLookForPlayerGoal` (~478-568): pending-target phase, `canContinueToUse` re-test, teleport-when-stared-at (<16 blocks), `teleportTowards` when the target is >256 away (~267). Needs a Steel-side subclass of `NearestAttackableTargetGoal`.
2. **NeutralMob persistent anger** (`isAngryAt`, anger timers, `ResetUniversalAngerTargetGoal`, PERSISTENT_ANGER_TIME 20-39 s). Steel has no `NeutralMob` system; today retaliation is covered by `HurtByTargetGoal`.
3. **Block pickup/place** (`EndermanTakeBlockGoal` / `EndermanLeaveBlockGoal`, ~428-605): needs `ENDERMAN_HOLDABLE` block tag (exists in the registry), the `carry_state` synced field (`SyncedValue<Option<BlockStateId>>`, already generated), `carriedBlockState` NBT in `save_additional`/`load_additional`, and the enchanted-diamond-axe loot drop for carried blocks (~320-337). Note AGENTS.md prefers `BlockRef` over raw `BlockStateId` in new generated data.
4. **Clean-water potion rule**: vanilla only takes the projectile branch for non-potion projectiles — a clean-water splash potion still hurts. Steel has no thrown-potion entity (only arrow / ender_pearl / firework / thrown_egg), so this cannot be mirrored yet.
5. **`hasIndirectPassenger` exclusion** in the stare check (~491, ~525).

## 5. Gotchas

- `SyncMutex` is parking_lot and NOT reentrant. `set_target` deliberately scopes the `entity_data` and `attributes` guards so they are never held at the same time (an earlier session fixed exactly this kind of self-deadlock in `pathfinder.rs` / `block_updates/mod.rs`).
- Do not implement vanilla behavior from memory — `minecraft-src/` is available; verify each item above against the cited files/methods first.
- The stare check runs every tick (like vanilla's overridden `canUse` — it does NOT use the `randomInterval` throttle), which is intended.
- `SyncedValue::set` only marks dirty on an actual value change, so resetting `creepy`/`stared_at` to false every tick is cheap.
- Steel's goal selector only ticks running goals that return `requires_update_every_tick() == true` (`mob/pathfinder.rs:187`); goals that need per-tick work must override it.
