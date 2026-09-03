#!/bin/bash
set -e

rm -f testserver/steel
cargo build
mv target/debug/steel testserver/steel

echo "done"
