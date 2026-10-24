# Getting Started

This guide gets `flint-steel` running locally against the SteelBenchmark test specs.

## 1. Install Prerequisites

- Rust with the toolchain from `rust-toolchain.toml`
- Git

Cargo downloads SteelMC and Flint dependencies from their git repositories during the first build.

## 2. Clone the Repository

```bash
git clone https://github.com/FlintTestMC/flint-steel.git
cd flint-steel
```

## 3. Verify the Harness Builds

Run a small local test that does not need the external benchmark specs:

```bash
cargo test --lib test_world_creation
```

## 4. Add Flint Test Specs

Clone SteelBenchmark into the default `./test` directory:

```bash
git clone https://github.com/FlintTestMC/SteelBenchmark.git test
```

To keep the tests somewhere else, set `TEST_PATH`:

```bash
TEST_PATH=/path/to/SteelBenchmark cargo test --lib test_run_flint_selected
```

## 5. Configure Test Filtering

Optional: copy the example config.

```bash
cp flint.toml.example flint.toml
```

`flint.toml` can select persistent filters:

```toml
viz_url = "http://localhost:7878"

[filter]
implemented_only = true

[filter.tags]
redstone = true
walls = false
```

Environment variables override config filters:

```bash
FLINT_TEST=place_fence cargo test --lib test_run_flint_selected
FLINT_PATTERN="*fence" cargo test --lib test_run_flint_selected
FLINT_TAGS=redstone,walls cargo test --lib test_run_flint_selected
```

## 6. Run the Benchmark Tests

Run the selected Flint tests:

```bash
cargo test --lib test_run_flint_selected -- --nocapture
```

Run every loaded benchmark spec:

```bash
cargo test --lib test_run_all_flint_benchmarks -- --nocapture
```

The test runner writes a CI summary to `log/flint_summary.json`.

## Failure Links

When a Flint assertion fails, the test output prints an `Open in flint-viz` URL. By default it points at:

```text
http://localhost:7878
```

Use `FLINT_VIZ_URL` or `viz_url` in `flint.toml` if your flint-viz instance runs elsewhere.
The TOML loader also accepts `flint_viz_url` and `FLINT_VIZ_URL`.
