# Kowloon

A courier's-eye game about Kowloon Walled City, from 1950 to 1987, in a small engine of its own (Rust + wgpu).

The city is grown, not built. A simulation accretes it year by year from a squatter village into the fourteen-storey block that was cleared in the 1990s. You walk it on foot: lanes, stairwells, corridors, bridges and rooftops. Every flat and shop has a named family or business that changes over the generations. You deliver parcels until you know the place, then watch it grow over what you learned.

> **Status: early alpha.** Rough edges, missing sound, and saves may break between versions.

## Play

**Download:** go to [Releases](../../releases), grab the latest `Kowloon-*-windows.zip`, unzip it anywhere, and run `kowloon.exe`. It needs Windows 10/11 and a GPU with DirectX 12 or Vulkan.

From the title screen:
- **Campaign:** start in 1950 as a courier. Learn the lanes, make deliveries from memory, and move on through the eras when you're ready.
- **Race:** 5 deliveries against the clock on a **course code** such as `KWC-1965-7F3A`. The same code gives the same city, the same era and the same jobs, so you can share one and race a friend. Your best run comes back as a ghost to race against. Choose **Rounds** (arrow and door names) or **Memory** (street plaques only). There's a new daily course every day.

| Key | |
|---|---|
| WASD, mouse | walk, look |
| Shift | jog |
| Space | hop, or vault low clutter |
| S on a ladder | slide down |
| E | knock |
| M / L | notebook map / ledger (campaign) |
| G | memory mode (campaign) |
| T / N | torch / night |
| R | run a race again |
| P | autopilot demo (campaign) |
| V | vsync on/off |
| Esc | menu |

Command line: `kowloon.exe --daily`, `--random`, `--code KWC-1965-7F3A [--memory]`, `--era 1970`, `--demo`.

## Build from source

You need Rust 1.95 or later.

```
cargo run --release -p kwc-app
cargo test --release --workspace
```

- `crates/kwc-sim`: the city and society simulation (headless, deterministic, tested).
- `crates/kwc-engine`: the renderer (wgpu), mesh, camera and GUI glue.
- `crates/kwc-app`: the game.

## About the place

Kowloon Walled City was real. By 1987 about 33,000 people lived and worked in 2.6 hectares, and it was demolished in 1993–94. This game is a work of imagination set there. Its layout is generated and every name in it is invented; it doesn't depict real residents. See [RESEARCH.md](RESEARCH.md) for what's grounded in sources and what's made up.

Site geometry uses data © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors (ODbL).

## Licence

© 2026 Chris. All rights reserved. You're welcome to play it and read the code, but please ask before reusing it.
