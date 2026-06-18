**ASK, DON'T GUESS** — Ambiguity is a stop signal. If types, error handling, or architecture is unclear, ask. No speculative coding.

**HONEST BLOCKERS** — Don't hide uncertainty behind speculative changes or workarounds. State the real issue and ask the user for missing logs, paths, data, or vanilla references.

**FOUNDATIONAL INTEGRITY** — Don't implement features on missing foundations. No stubs, mocks, or hardcoded values (`todo!()`, `const user = {id:1}`) unless explicitly prototyping.

**VANILLA FUNCTIONALITY** - Keep 1:1 Vanilla functionality. If a faster or more idiomatic Rust solution diverges from vanilla behavior or structure, get explicit permission first and leave a concise comment/doc explaining why.

**SKETCHY WORKAROUND PROTOCOL** — Halt and ask permission before using:
- `.clone()` to appease borrow checker, `.unwrap()`/`.expect()` in production, `unsafe`
- `Any` (Any isn't abi safe and a bad workaround)
- Disabling linters, ignoring errors, deprecated deps

Template: *"This requires [Hack] which risks [Consequence]. Proceed or solve root cause?"*

**CONSTRUCTIVE DISSENT** — Treat user claims as hypotheses. Verify against local code/vanilla, call out mismatches, and challenge XY problems. Before building a complex system, identify the required vanilla-compatible foundations and ask when architecture is unclear.

**Registries**
 - We should only generate what is needed. Does minecraft use a hardcoded transform? Then we do as well.
 - We should design everything with modding and abi compatibility in mind for the future. No requirement to add it while writing but it has to be thought out to be extendable. Every value/registry neoforge can change we should be able to change in the future
 - Vanilla extracted registry/worldgen data should be compiled by build scripts into typed Rust data, not parsed from JSON at runtime. Prefer generated references like `vanilla_blocks::STONE`/`vanilla_fluids::WATER` and registry ref types over `Identifier` when the referenced vanilla value is known.
 - Do not design for runtime datapack JSON loading. Future plugins should register their own typed blocks/features with their own refs; build scripts only need to generate vanilla data.
 - Avoid raw `BlockStateId` in generated registry data. Use `BlockRef` plus explicit properties/default-state resolution so registry ordering can still evolve for plugin support.

**Code standard**
 - Use vanilla names unless they are misleading or a Rust design is explicitly approved. Document intentional differences on the relevant struct, method, or module.
 - We should try to minimize code duplication, but a few lines are usually fine.
 - Treat foundational systems with extra rigor; shortcuts in shared interfaces like block behavior become expensive once implementations depend on them.
 - No workarounds. Create the right helper/abstraction when the code needs one.
 - Don't add trivial wrapper methods that just alias an existing method. If `height()` already exists, don't add `get_y_size()` that returns `self.height()`. Use the existing method directly.
 - Prefer associated functions on the relevant type over standalone free functions when there's a clear owner.
 - Avoid deep indentation; prefer guard clauses, `if let`, and `let Some(...) = ... else { return }`.
 - Use `Result` for recoverable failures; panic only for impossible or fatal states.
 - Don't multithread something unless you can explain why it needs multithreading.
 - Don't use async unless you need disk or network I/O.
 - If you haven't fully implemented a feature, add a `// TODO:` comment.
 - Keep comments concise
 - After fixing something don't leave a comment
 - Currently this project is in early development, we don't need to provide migrations

 **Testing**
  - Add tests for advanced systems, code using unsafe (Always use // SAFETY comments) or code that needs to match vanilla determinism (ItemComponent hashing or worldgen)
  - Suppress clippy lints with `#[expect(clippy::lint_name, reason = "...")]`. False positives and intentional deviations (e.g., function length for readability) are acceptable when explained

**GENERATED CODE** — Never modify generated files directly:
- `steel-registry/src/generated/` → modify `steel-registry/build/`
- `steel-core/src/behavior/generated/` → modify `steel-core/build/`
- `steel-worldgen/src/generated/` → modify `steel-worldgen/build/`
- `steel-utils/src/generated/` → modify `steel-utils/build/`
- Block/item behavior registration is generated from `#[block_behavior]` / `#[item_behavior]`; add annotated structs under `steel-core/src/behavior/`, not manual generated registration.
- Treat `*/build_assets/*.json` and `*/test_assets/*.json` populated by SteelExtractor as extracted data, not hand-authored source. If extracted JSON data is wrong or missing, update the relevant SteelExtractor extractor, rerun it, and copy only the produced file(s) needed for the change.
- If extracted JSON data is missing and the extractor path/output is not available, tell the user exactly what data is required; they can provide the extractor path.

## Build Commands

```bash
cargo build          # Build
cargo run            # Run
cargo check          # Fast compile check
cargo test           # Tests
cargo clippy -r --all-targets --all-features  # CI lint
cargo check -p steel-core          # Fast game/worldgen check
```

Uses **nightly Rust**.
Tooling: `ast-grep` is available for structural code search/rewrites.

## Architecture

Steel = Minecraft 26.1 server in Rust.

**Crates:** `steel` (thin wrapper) -> `steel-login` (initial connection) → `steel-core` (game logic) → `steel-worldgen` (worldgen) → `steel-protocol` (packets) → `steel-macros` (derives) → `steel-registry` (generated data) → `steel-utils`/`steel-math` (common) → `steel-crypto` (encryption)

## Packets
Serverbound = `ReadFrom`, Clientbound = `WriteTo`.

```rust
// Serverbound
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SSwing { pub hand: InteractionHand }

// Clientbound
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_ANIMATE)]
pub struct CAnimate {
    #[write(as = VarInt)]
    pub entity_id: i32,
    pub action: AnimateAction,
}
```

**Attributes:** `#[read/write(as = VarInt)]`, `#[read(as = Prefixed(VarInt))]`

**After creating:** Export in `steel-protocol/src/packets/game/mod.rs`, add handler in `steel-core/src/player/networking.rs`

## Key Paths

**Steel:**
| Area | Path |
|------|------|
| Entry / networking | `steel/src/` |
| Game logic | `steel-core/src/` |
| Player | `steel-core/src/player/` (`networking.rs` = packet handlers) |
| World | `steel-core/src/world/` |
| Worldgen orchestration | `steel-core/src/worldgen/` |
| Worldgen algorithms/data | `steel-worldgen/src/` |
| Chunks | `steel-core/src/chunk/` |
| Block/item behaviors | `steel-core/src/behavior/` |
| Block entities | `steel-core/src/block_entity/` |
| Entities | `steel-core/src/entity/` |
| Inventory / menus | `steel-core/src/inventory/` |
| Packets | `steel-protocol/src/packets/game/` |
| Codegen build scripts | `steel-core/build/`, `steel-registry/build/`, `steel-worldgen/build/`, `steel-utils/build/` |

**Vanilla** (`minecraft-src/minecraft/`):
| Area | Path |
|------|------|
| Source root | `src/net/minecraft/` |
| Packet handlers | `src/.../server/network/ServerGamePacketListenerImpl.java` |
| Blocks | `src/.../world/level/block/` |
| Entities | `src/.../world/entity/` |
| Player | `src/.../server/level/ServerPlayer.java` |
| Worldgen region | `src/.../server/level/WorldGenRegion.java` |
| Worldgen features/structures | `src/.../world/level/levelgen/` |

The `minecraft-src/` folder should be generated by `update-minecraft-src.sh`. If it isn't, stop and state the issue to the user.

## Concurrency Quick Ref
- `steel_utils::locks::SyncMutex<T>` / `steel_utils::locks::SyncRwLock<T>` — aliased parking_lot for easier comprehension
- `Weak<T>` — prevent reference cycles
- If you have related values bundle them over a struct that is put behind a `SyncMutex`
