# Contract: Services

```rust
// provider (pre-init or init)
services::register::<DynEconomy>("vault:economy/v1", 10, Box::new(EssentialsEconomy).into())?;
// consumer (any time after init)
let economy: &'static DynEconomy = services::get("vault:economy/v1")?;  // Err if none/incompatible
let all = services::providers("vault:economy/v1");                       // Vec<ServiceInfo>
```

- `T` must be `IStable + Send + Sync + 'static`; contracts are stabby traits wrapped by
  `stabby::dynptr!(Box<dyn Contract + Send + Sync>)` in a shared API crate.
- Host stores `{key, provider, priority, report, object}`; lookup compares `T::REPORT` with the
  stored report and fails with `incompatible service provider` on mismatch.
- Objects are pinned for process lifetime (providers are never unloaded).
- Duplicate registration of the same key by the same plugin replaces nothing and errors.
- Keys are versioned (`/v1`); a breaking contract change publishes a new key.

## Contracts shipped

- `vault:economy/v1`, `vault:permission/v1`, `vault:chat/v1` (crate `vault-api`)
- `worldedit:selection/v1` (crate `worldedit-api`): `selection(player) -> Option<Cuboid>` where
  `Cuboid { world: String, min: BlockPos, max: BlockPos }`.
