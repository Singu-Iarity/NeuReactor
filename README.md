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

## Equipment and upgrades

The **Equipment** tab lists the unit's I&C hardware and automation software.
- **Computers** run automation.
- **Panel packages** modernise one area of the board and connect it to its
  computer.
- **Automation modules** need their computer *and* panel package to run.

Only the board sections a package covers change to the modern look, so you
can, for example, run a modern turbine-generator section next to original
analog reactor panels. In sandbox, toggle each item freely, or use the
Plant Computer tab presets ORIGINAL PLANT / FULLY MODERNISED. The sandbox
starts fully modernised.

## Career mode

Open the **Career** tab and choose a start year and difficulty.

1. **Construction.** You get a budget scaled to the era. Equip all nine plant
   components:
   - turbine, generator, condenser, feed pumps, reactor coolant pumps, steam
     generators, rod drives, pressurizer, board instruments
   - each in an **era** (vintage / dated / modern) and a **tier** (basic /
   quality / premium). Better parts mean more MWe, more flow, faster rods,
   less instrument noise.
   - choose each automation module as an **analog cabinet** (no computer,
   less precise) or **digital software** (needs its computer + panel
   package), and buy computers and panel packages.
   - equipment two eras apart from what it connects to (e.g. a modern
   turbine on a vintage generator, or a modern digital feedwater controller
   on vintage feed pumps) needs a **conversion kit**, priced into the
   purchase automatically.
2. **Commission** the unit. It starts at hot standby with every module in
   MANUAL. Power sold earns $/MWh; O&M costs money every day.
3. **Calendar & research.** As the in-game years pass, new technology
   arrives ("due to new research, you can now install …"). Pay for research
   projects to unlock it early. Major components can only be swapped during
   an outage; controllers and I&C hardware anytime.

Use the 1000× / 5000× time buttons to let research and calendar time pass.
There is no save game yet; a career lasts as long as the server runs.

## Playing: a reactor startup

The server starts the plant at **hot standby**: no-load temperature and
pressure, all rods inserted, the turbine on turning gear. (Use
`--scenario full_power` to start at power instead, or the Plant Computer tab's
instructor station.) The tabs are panels of the control board, and each one
has its own annunciator. ACK/SIL/RST acknowledge, silence and reset alarms.

1. **Reactor tab: shutdown banks.** Set BANK SELECT to SA and hold the ROD
   MOTION lever on OUT until SA reaches 228 steps. Repeat for SB.
2. **Approach criticality.** Set BANK SELECT to MAN, which moves the control
   banks in overlap. Withdraw in steps while you watch the **source range**
   count rate and startup rate. Turn on SR SPEAKER to hear the counts.
3. **Block the source range at P-6.** Once the intermediate range is above
   1E-10 A (P-6 lamp), press **SR BLOCK**. If you don't, the SR high-flux trip
   at 1E5 cps scrams the reactor.
4. **Up the intermediate range to P-10.** Keep the startup rate under about
   1 dpm. When 2 of the 4 power range channels exceed 10 % (P-10), press
   **IR BLOCK** and **PR LO BLOCK** before 25 %. Otherwise those trips fire.
5. **Turbine-Generator tab.**
   - Press TURBINE RESET and close the EXCITER FIELD BKR.
   - Pick an ACCELERATION and press 1800 to roll the turbine to speed.
6. **Synchronize.**
   - Trim the governor so the synchroscope turns slowly in the FAST
     direction.
   - Close the GEN BREAKER as the pointer reaches 12 o'clock.
   - On the analog board an out-of-phase close damages the generator and
     trips it. Badly damaged, it locks out (86G) until repaired.
   - With the Auto-Synchronizer in AUTO, or the Turbine Control Computer plus
     the Generator Sync Interface installed, the sync-check relay blocks bad
     closes and lights **PHASE Δ OUT OF RANGE**. In AUTO it also
     synchronizes by itself.
7. **Load the unit.** Raise the governor's MWe setpoint and put rod control
   in AUTO when ready.

## What's simulated today (PWR)

- Point kinetics with 6 delayed-neutron groups (implicit, stable at any step)
- Doppler, moderator, boron, rod (S-curve), xenon-135 and samarium-149 reactivity
- 9-group decay heat fitted to Way–Wigner
- Fuel heat capacity, 4-node primary loop, RCP coastdown and natural circulation
- Pressurizer: insurge compression, heaters, sprays, PORV and safety valves
- Steam generators, turbine/generator, steam dumps, relief valves, feedwater heating
- Westinghouse-style nuclear instrumentation: source, intermediate and power range, with P-6/P-10 blocks
- Turbine-generator: shaft dynamics, grid frequency drift, synchroscope phase, excitation, sync-check, damage and lockout
- Reactor protection system (NIS, pressure, level, flow, ΔT, SG level, turbine trip)
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
| `web/` | Control room UI: tabbed panels, annunciators, switchgear widgets; analog and digital themes (plain ES modules, embedded at build time) |
| `docs/` | Architecture, physics notes, roadmap, API reference |
| `examples/plugins/` | Example third-party plugin |

See [docs/ROADMAP.md](docs/ROADMAP.md) for what comes next and
[docs/api/README.md](docs/api/README.md) to write plugins.
