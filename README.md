# NeuReactor

A standalone, high-fidelity nuclear power plant simulator. It aims to work for
beginners, who can lean on automatic regulation, and for experienced operators,
who can control almost everything by hand. A career/economy mode is planned:
start with a bare-bones unit, sell power to the grid, buy automation modules,
instrumentation upgrades and spare parts, fix malfunctions, and grow to a
multi-unit, multi-gigawatt site.

**Status:** early development (milestones M0–M1). There's a playable
**four-loop PWR sandbox** with a browser control room and a plugin API.

## Quick start

```bash
cargo run --release -p nr-server
```

Then open <http://127.0.0.1:8642/>. The simulation runs natively in that one
small binary. Your browser is only the control room display, so there's no
Electron and no heavy runtime.

Headless runs and benchmarks:

```bash
cargo run --release -p nr-cli -- scenario scram --seconds 900 --every 10 > scram.csv
cargo run --release -p nr-cli -- bench
```

Scenarios: `steady`, `scram`, `load-follow`, `rcp-trip`, `xenon`.

## What's simulated today (PWR)

- Point kinetics with 6 delayed-neutron groups (implicit, stable at any step)
- Doppler, moderator, boron, rod (S-curve), xenon-135 and samarium-149 reactivity
- 9-group decay heat fitted to Way–Wigner
- Fuel heat capacity, 4-node primary loop, RCP coastdown and natural circulation
- Pressurizer: insurge compression, heaters, sprays, PORV and safety valves
- Steam generators, turbine/generator, steam dumps, relief valves, feedwater heating
- Reactor protection system (flux, pressure, level, flow, ΔT, SG level, turbine trip)
- Instruments separate from true values (lag, noise, faults)
- **Per-subsystem automation modules**: rod control, boron, pressurizer
  pressure and level, feedwater, steam dump, turbine governor, generator sync,
  plant master, and an external control gateway for plugins. Each one is
  switched on its own between MANUAL, AUTO and EXTERNAL.

## Repository layout

| Path | Purpose |
|---|---|
| `crates/nr-core` | Numerics, PI controllers, steam tables, deterministic RNG |
| `crates/nr-physics` | Reactor-agnostic physics components and sensors |
| `crates/nr-api` | Commands, telemetry, module catalog, plugin scopes and access policy |
| `crates/nr-control` | Automation controllers and the reactor protection system |
| `crates/nr-reactors` | Plant models (PWR now; BWR, RBMK and fast breeder later) |
| `crates/nr-cli` | Headless scenarios and benchmarks |
| `crates/nr-server` | Local server: runs the sim, serves the UI and the `/api/v1` plugin API |
| `web/` | Control room UI (plain HTML/CSS/JS, embedded in the server binary) |
| `docs/` | Architecture, physics notes, roadmap, API reference |
| `examples/plugins/` | Example third-party plugin |

See [docs/ROADMAP.md](docs/ROADMAP.md) for what comes next and
[docs/api/README.md](docs/api/README.md) to write plugins.
