**ASK, DON'T GUESS** — Ambiguity is a stop signal. If types, error handling, or architecture is unclear, ask. No speculative coding.

**FOUNDATIONAL INTEGRITY** — Don't implement features on missing foundations. No stubs, mocks, or hardcoded values (`todo!()`, `const user = {id:1}`) unless explicitly prototyping.

**VANILLA FUNCTIONALITY** - We should keep 1:1 Vanilla functionality. If a compromise can be made you MUST present the issue at hand to the user first.

**SKETCHY WORKAROUND PROTOCOL** — Halt and ask permission before using:
- `.clone()` to appease borrow checker, `.unwrap()`/`.expect()` in production, `unsafe`
- `Any` (Any isn't abi safe and a bad workaround)
- Disabling linters, ignoring errors, deprecated deps

Template: *"This requires [Hack] which risks [Consequence]. Proceed or solve root cause?"*

**CONSTRUCTIVE DISSENT** — Challenge XY problems. *"I can do X, but it introduces [Issue]. Standard pattern is Y. How proceed?"*

**Registries**
 - We should only generate what is needed. Does minecraft use a hardcoded transform? Then we do as well.
 - We should design everything with modding and abi compatibility in mind for the future. No requirement to add it while writing but it has to be thought out to be extendable. Every value/registry neoforge can change we should be able to change in the future

**Code standard**
 - Usually vanilla is decent at naming stuff, sometimes we want to deviate from this though in cases where names are bad or non descriptive. Or we want a whole other solution to the system at hand. In that case we should add a doc comment above the struct, method or module that clearly states the differences so next time someone new picks it up they have an easy time understanding your system.
 - We should try to minimize code duplication, but a few lines are usually fine.
 - When working on foundation we must be extra sure we aren't taking any shortcuts or leaving stuff out, this can cause issues later down the line where a foundational system has to be completely redesigned. Foundational code is code like a system or interface other code depends on, an example being the block behavior trait, if that's badly designed from the start and we have 100 block implementations building off it good luck getting it changed.
 - No workarounds. Don't be lazy and skip creating a helper function just cause you only needed it once for your use case.
 - Don't add trivial wrapper methods that just alias an existing method. If `height()` already exists, don't add `get_y_size()` that returns `self.height()`. Use the existing method directly.
 - Try to not go deep in indentation, guard clauses are useful for this and rust has some really nice `if let` and `let Some() = x else {return}`
 - Don't use panics unless the case never happens or is fatal to the program. Otherwise use Results
 - Don't multithread something unless you can explain why it needs multithreading.
 - Don't use async unless you need disk or network I/O.
 - If you haven't fully implemented a feature, make sure to add a // TODO: comment
 - Keep comments concise
 - After fixing something don't leave a comment

 **Testing**
  - Add tests for advanced systems, code using unsafe (Always use // SAFETY comments) or code that needs to match vanilla determinism (ItemComponent hashing or worldgen)
  - Only #[allow] clippy lints with a justification comment unless obvious. False positives and intentional deviations (e.g., function length for readability) are acceptable when explained.

**GENERATED CODE** — Never modify generated files directly:
- `steel-registry/src/generated/` → modify `steel-registry/build/build.rs`
- `steel-core/src/behavior/generated/` → modify `steel-core/build/items.rs` or `steel-core/build/blocks.rs`
- When some data is missing from the extracted json files ALWAYS present the user with what data is required and they can provide you with a path of where the extractor code is.

## Build Commands

```bash
cargo build          # Build
cargo run            # Run
cargo check          # Fast compile check
cargo test           # Tests
cargo clippy --fix --allow-dirty  # Lint
```

Uses **nightly Rust**.

## Architecture

Steel = Minecraft 1.21.11 server in Rust.

**Crates:** `steel` (thin wrapper) -> `steel-login` (initial connection) → `steel-core` (game logic) → `steel-protocol` (packets) → `steel-macros` (derives) → `steel-registry` (generated data) → `steel-utils` (common) → `steel-crypto` (encryption)

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
| Chunks | `steel-core/src/chunk/` |
| Block/item behaviors | `steel-core/src/behavior/` |
| Block entities | `steel-core/src/block_entity/` |
| Entities | `steel-core/src/entity/` |
| Inventory / menus | `steel-core/src/inventory/` |
| Packets | `steel-protocol/src/packets/game/` |
| Codegen build scripts | `steel-core/build/`, `steel-registry/build/` |

**Vanilla** (`minecraft-src/minecraft/`):
| Area | Path |
|------|------|
| Source root | `src/net/minecraft/` |
| Packet handlers | `src/.../server/network/ServerGamePacketListenerImpl.java` |
| Blocks | `src/.../world/level/block/` |
| Entities | `src/.../world/entity/` |
| Player | `src/.../server/level/ServerPlayer.java` |

The `minecraft-src/` folder should be generated by `update-minecraft-src.sh`. If it isn't, stop and state the issue to the user.

## Concurrency Quick Ref
- `steel_utils::locks::SyncMutex<T>` / `steel_utils::locks::SyncRwLock<T>` — aliased parking_lot for easier comprehension
- `Weak<T>` — prevent reference cycles
- If you have related values bundle them over a struct that is put behind a `SyncMutex`
