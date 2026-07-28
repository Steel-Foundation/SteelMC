<p align="center">
  <img src=".github/assets/readme/steel-logo.png" alt="SteelMC logo" width="192">
</p>

<h1 align="center">SteelMC</h1>

<p align="center">
  A Minecraft Java Edition server written in Rust, built for vanilla-compatible behavior,
  parallelism, performance, and foundations that last.
</p>

<p align="center">
  <a href="https://github.com/Steel-Foundation/SteelMC/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/Steel-Foundation/SteelMC?display_name=tag&sort=semver&style=flat-square"></a>
  <a href="https://github.com/Steel-Foundation/SteelMC/actions/workflows/test.yml"><img alt="Tests" src="https://img.shields.io/github/actions/workflow/status/Steel-Foundation/SteelMC/test.yml?branch=master&label=tests&style=flat-square"></a>
  <a href="https://github.com/Steel-Foundation/SteelMC/actions/workflows/lint.yml"><img alt="Lint" src="https://img.shields.io/github/actions/workflow/status/Steel-Foundation/SteelMC/lint.yml?branch=master&label=lint&style=flat-square"></a>
  <a href="LICENSE"><img alt="AGPL-3.0-or-later license" src="https://img.shields.io/github/license/Steel-Foundation/SteelMC?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://steelmc.dev/">Website</a> ·
  <a href="https://steelmc.dev/getting-started/introduction/">Documentation</a> ·
  <a href="https://steelmc.dev/tracker">Implementation tracker</a> ·
  <a href="https://steelmc.dev/discord">Discord</a>
</p>

![A sunset over a SteelMC-generated world, with forests, rivers, mountains, and a lit village](.github/assets/readme/sunset.webp)

> [!IMPORTANT]
> SteelMC is in pre-alpha. You can connect, explore generated worlds, and test the
> systems already in place, but survival gameplay and vanilla parity are not complete.
> Do not replace a production server with SteelMC yet.

## What is SteelMC?

SteelMC is an independent implementation of the Minecraft Java Edition server. The
current release line targets **Minecraft Java Edition 26.2**.

Clients can join a persistent multiplayer world, move and interact, use inventories
and commands, and return to chunks that have been saved. SteelMC is already more than
a protocol or world-generation demo, but it remains early enough that testing,
feedback, and contributions make a meaningful difference.

The project is guided by three priorities:

- **Vanilla-compatible behavior.** SteelMC studies the vanilla implementation and
  preserves behavior players depend on, including useful quirks, instead of treating
  compatibility as a surface-level protocol target.
- **Parallel foundations.** Chunk scheduling, generation, lighting, packet
  processing, and chunk sending are designed to use modern hardware without giving
  up well-defined synchronous points for gameplay logic.
- **Maintainable systems.** Shared foundations are designed around the complicated
  cases they will eventually need to support, keeping the codebase pleasant to work
  in as the server grows.

## Something real to test

SteelMC's terrain-generation output has matched vanilla block for block across
**7,500 randomly selected test chunks**: 2,500 in each dimension. The comparison
uses a reproducible vanilla reference with entity spawning excluded and a small
number of order-dependent behaviors normalized.

The world generator also scales across many CPU cores. In a focused benchmark on a
Ryzen 9 9950X, SteelMC generated a fresh 10,201-chunk Overworld area in a median of
3.96 seconds. This is a world-generation benchmark, not a claim about every server
workload.

Read [Introducing SteelMC](https://steelmc.dev/blog/announcement/) for the design
story, parity methodology, benchmark context, and limitations. Full results and
reproduction instructions are available on the
[benchmark page](https://steelmc.dev/reference/benchmarks/).

## Current status

SteelMC currently includes:

- Java Edition networking, authentication, encryption, and compression
- Persistent chunk generation, loading, saving, and lighting
- Player movement, collision, block interaction, and inventories
- Commands, permissions, chat, and server configuration
- Early entity, block entity, and gameplay behavior implementations

Important limitations:

- Survival gameplay is incomplete.
- Only a small number of entities have meaningful behavior.
- Full vanilla and protocol parity have not been reached.
- Plugins are not available yet.
- Paper, Bukkit, Fabric, Forge, and NeoForge extensions are not compatible.

Follow the [implementation tracker](https://steelmc.dev/tracker) for a more detailed
view of what is available today.

## Try SteelMC

Pre-built releases, Docker images, and source-build instructions are available in
the [installation guide](https://steelmc.dev/getting-started/installation/).

Expect bugs and incomplete mechanics. If you try SteelMC, please share what you find
on [Discord](https://steelmc.dev/discord) or open a
[GitHub issue](https://github.com/Steel-Foundation/SteelMC/issues).

## Contributing

SteelMC is built by people who care about understanding Minecraft's behavior and
turning that understanding into clear, reliable Rust. New contributors are welcome.

Before starting:

1. Check existing issues and pull requests, then discuss substantial changes with
   the community.
2. Read the [contributor guide](https://steelmc.dev/development/start-contributing/)
   and [code standards](https://steelmc.dev/development/code-standard/).
3. Generate the targeted vanilla source with `./update-minecraft-src.sh` and verify
   behavior against it.
4. Run the relevant tests and checks before opening a pull request.

The repository uses a pinned nightly Rust toolchain. The common validation commands
are:

```bash
cargo test
cargo fmt --all --check
cargo clippy -r --all-targets --all-features
typos
```

AI may be used as a tool, but contributors must understand and be able to explain
every line they submit. Fully autonomous pull requests are not accepted.

## Community

Questions, design discussions, progress updates, and contributor coordination happen
on the [SteelMC Discord](https://steelmc.dev/discord). You can also follow project
updates on the [SteelMC website](https://steelmc.dev/).

## License

SteelMC is free software licensed under the
[GNU Affero General Public License v3.0 or later](LICENSE).

The SteelMC logo was designed by **colonthreeing**.

## Acknowledgements

SteelMC has been inspired by the work of
[C2ME](https://github.com/RelativityMC/C2ME-fabric),
[ScalableLux](https://github.com/RelativityMC/ScalableLux),
[FastNoise](https://codeberg.org/ZenXArch/FastNoise),
[Lithium](https://github.com/CaffeineMC/lithium), and
[Structure Layout Optimizer](https://github.com/TelepathicGrunt/StructureLayoutOptimizer).

## Top contributors

<a href="https://github.com/Steel-Foundation/SteelMC/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=Steel-Foundation/SteelMC" alt="SteelMC contributors">
</a>
