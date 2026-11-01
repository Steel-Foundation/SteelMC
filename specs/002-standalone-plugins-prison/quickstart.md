# Quickstart: OP Prison demo

```sh
cargo build --release -p steel
cargo build --release --manifest-path plugin-projects/Cargo.toml
mkdir -p run/plugins && cp plugin-projects/target/release/lib{vault,essentials,worldedit,worldguard,prison_mines,prison_enchants}.so run/plugins/
cd run && ../target/release/steel
```

In game (operator):
1. `//wand`, select a 50×50×50 box, `/mine create a`, `/mine set a 60%stone,25%coal_ore,10%iron_ore,5%diamond_ore`, `/mine reset a`
   → chat shows reset time and blocks/s.
2. `/pickaxe`, right-click → enchant menu; `/eco give <you> 1000000`, buy Jackhammer levels.
3. Mine inside the mine: layers vanish, particles, balance grows (sidebar).

## Automated
```sh
cargo test -p steel-core --lib bulk_fill
cargo test -p steel-plugin-api --features host -- --include-ignored
cargo test --manifest-path plugin-projects/Cargo.toml
cargo test --release -p steel-core --lib bulk_fill_benchmark -- --ignored --nocapture
```

Recorded (T019): 125 000 blocks in 11.3 ms and 1 000 000 blocks in 94.2 ms with the bulk fill
(10.6 M blocks/s), against 72.6 ms and 641.8 ms with per-block `set_block`.
