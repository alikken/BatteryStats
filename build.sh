#!/usr/bin/env bash
# Сборка BatteryStats.exe и установщика. Версия берётся из Cargo.toml.
# Нужны: Rust с целью x86_64-pc-windows-gnu, mingw-w64 (gcc + windres), NSIS 3 (makensis).
set -euo pipefail
cd "$(dirname "$0")"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release
EXE="$PWD/target/x86_64-pc-windows-gnu/release/BatteryStats.exe"
mkdir -p dist
cp "$EXE" dist/BatteryStats.exe
( cd installer && makensis -V2 -DVERSION="$VERSION" -DAPP_EXE="$EXE" -DOUTFILE="$PWD/../dist/BatteryStats-Setup-$VERSION.exe" installer.nsi )
ls -la dist
