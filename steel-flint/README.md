# flint-steel

Flint testing framework integration for [SteelMC](https://github.com/Steel-Foundation/SteelMC). This crate implements the Flint traits (`FlintAdapter`, `FlintWorld`, `FlintPlayer`) so that automated Minecraft tests run against the real SteelMC server logic — no mocks, 100% code reuse with production behavior.

## What This Crate Provides

- A `SteelAdapter` implementation for Flint's test runner
- A `SteelTestWorld` backed by the real `steel-core` world with RAM-only storage
- A `SteelTestPlayer` that drives real inventory, item, and block interaction code
- Test helpers for running JSON Flint specs from SteelBenchmark
- Failure links for opening test failures in `flint-viz`

## How It Works

flint-steel wraps real `steel-core` internals in lightweight test harnesses:

- **SteelTestWorld** — wraps an `Arc<World>` with RAM-only storage and on-demand empty chunks (no disk I/O)
- **SteelTestPlayer** — uses real inventory, item, and block behavior with a mock network connection that records packets
- **SteelAdapter** — creates test worlds and drives the Flint test runner

Tests are written as JSON specs (see [SteelBenchmark](https://github.com/FlintTestMC/SteelBenchmark)) and executed against the real game engine, including block neighbors, shapes, callbacks, and tick processing.

## Discord

Join the [Flint Discord](https://discord.gg/kJXKfgx66X) to discuss this project or ask questions.

## Prerequisites

- Rust (edition 2024)
- Git
- [SteelBenchmark](https://github.com/FlintTestMC/SteelBenchmark) test specs

`steel-core`, `steel-protocol`, `steel-registry`, `steel-utils`, and `flint-core` are Cargo git dependencies and are resolved automatically during the build.

## Installation

```bash
git clone https://github.com/FlintTestMC/flint-steel.git
cd flint-steel
cargo test --lib test_world_creation
```

For a full first run, continue with [GETTING_STARTED.md](GETTING_STARTED.md).

## Getting Tests

Clone [SteelBenchmark](https://github.com/FlintTestMC/SteelBenchmark) to provide the test files:

```bash
git clone https://github.com/FlintTestMC/SteelBenchmark.git test
```

By default, tests are loaded from `./test`. You can change this with the `TEST_PATH` environment variable.

You can also copy the example Flint config:

```bash
cp flint.toml.example flint.toml
```

## Usage

```bash
# Run all Flint tests
cargo test --lib

# Run a single test by name
FLINT_TEST=place_fence cargo test --lib

# Run tests matching a glob pattern
FLINT_PATTERN="*fence" cargo test --lib

# Run tests filtered by tags
FLINT_TAGS=redstone,walls cargo test --lib
```

### Environment Variables

| Variable | Default | Description |
|---|---|---|
| `FLINT_TEST` | — | Run a single test by exact name |
| `FLINT_PATTERN` | — | Run tests matching a glob pattern |
| `FLINT_TAGS` | — | Filter tests by comma-separated tags |
| `TEST_PATH` | `./test` | Path to test files directory |
| `FLINT_VIZ_URL` | `http://localhost:7878` | Base URL used when printing flint-viz links for failures; overrides TOML |

You can set these in a `.env` file. `flint.toml` supports persistent test filtering:

```toml
viz_url = "http://localhost:7878"

[filter]
implemented_only = true

[filter.tags]
redstone = true
walls = false
```

Environment variables take priority over `flint.toml`. The viz URL can be set in TOML as `viz_url`, `flint_viz_url`, or `FLINT_VIZ_URL`.

## Documentation

- [GETTING_STARTED.md](GETTING_STARTED.md) - first-run setup and common commands
- [CONTRIBUTING.md](CONTRIBUTING.md) - development workflow and contribution guidelines

## Related Projects

| Project | Description |
|---|---|
| [SteelMC](https://github.com/Steel-Foundation/SteelMC) | SteelMC server implementation |
| [flint-core](https://github.com/FlintTestMC/flint-core) | Flint testing framework |
| [SteelBenchmark](https://github.com/FlintTestMC/SteelBenchmark) | Test suite and benchmark specs |
| [FlintCLI](https://github.com/JunkyDeveloper/FlintCLI) | Command-line interface for Flint |
| [FlintDocs](https://github.com/JunkyDeveloper/FlintDocs) | Documentation |

## License

[MIT](LICENSE)
