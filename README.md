# cosmic-layout-applet

A small third-party COSMIC panel applet for Pop!_OS 24.04 that shows the current keyboard layout.

This applet was made as a workaround for cases where the built-in COSMIC layout indicator does not reflect the real layout correctly.

## Features

- Shows current layout in the COSMIC panel
- Minimal text indicator (`US` / `RU`)
- Designed for Pop!_OS 24.04 + COSMIC
- Works as a standalone COSMIC applet

## Status

This project is an experimental workaround.

It currently targets setups with custom layout switching such as `Alt+Shift` and may not be perfect in all cases.

## Build

Install dependencies, then build with Cargo:

```bash
cargo build --release

bash scripts/install-local.sh
