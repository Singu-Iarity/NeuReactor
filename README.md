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

## Automatic plant control

- **Reactor Power Controller** (Reactor tab), with rod control in AUTO:
  - ROD: rods follow a power setpoint, startup-rate limited.
  - BORON: dilute/borate with rods parked.
  - COORD: rods for fast changes, boron returns the lead bank to its band.
  - Analog cabinets support ROD only.
- **Unit Load Controller** (plant master, Grid tab): drives the turbine load
  target. With the **Grid Dispatch Link** installed, **AGC** follows grid
  demand automatically.
- **Automatic rod balancer:** part of rod control (OFF/S/M/F). It realigns
  rod groups that slip apart and tilt the core's power. With the **Rod Group
  Select** pushbutton grid you can also move single groups by hand.

## Sound

Everything has sound, synthesised in the browser. Audio starts after your
first click (a browser rule).
- **Switches:** J-handle switches clunk, selector knobs click, pushbuttons
  and spring switches snap. Rejected commands buzz.
- **Plant events:**
  - breakers clack open and closed
  - the turbine stop valves slam
  - a scram clatters as the rods drop
  - control rods tick with every step
  - reactor coolant pumps growl when they start
- **Ambience:** HVAC, the hum of the coolant pumps, and a turbine whine that
  follows shaft speed, plus steam dump/relief roar and PORV hiss.
- **Alarms:** warnings chime, alarms sound the annunciator horn, trips sound a
  klaxon. ACK and SIL behave as on a real board.
- **SR SPEAKER** (Reactor tab) plays the source range count rate.

Master volume and mute are in the top bar. Ambience, alarm and effects
toggles are on the Plant Computer tab, and the settings are remembered.

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
   MANUAL. Power sold earns $/MWh.
   - **Monthly bills** accrue as you run and are charged at month end:
     nuclear fuel (thermal energy), cooling water (per CW pump), grid power
     imported for station service while the generator is off-line,
     maintenance and staff. Statements appear in the Career tab and news feed.
   - **Penalties**: an unplanned loss of generation (trip, reverse-power
     relay, loss of offsite power) is fined in proportion to the MWe lost.
     Opening the breaker yourself is not penalised.
   - **Diesel fuel**: emergency diesels burn fuel while running; order a
     delivery from the Electrical tab.
3. **Calendar & research.** As the in-game years pass, new technology
   arrives ("due to new research, you can now install …"). Pay for research
   projects to unlock it early. Major components can only be swapped during
   an outage; controllers and I&C hardware anytime.

4. **Unit size.** The reactor core sets the unit size: 600 MWe (121 fuel
   assemblies, 29 control rods), 900 MWe (157, 41), 1150 MWe (193, 53) or
   1300 MWe (241, 69). Sizes arrive over the years (600 MWe 1965, 900 MWe
   1968, 1150 MWe 1972, 1300 MWe 1982).
   - Every other component (except the board instruments) also comes in a
     size. Anything smaller than the core derates the unit to that
     component's rating; rod drives smaller than the core cannot move its
     rods.
   - A bigger core is bought in an outage. The unit is rebuilt at the new
     size at hot standby, and the plant computers and automation need an
     I&C rescaling package (priced into the purchase). Coupled components
     of different sizes need size adapters.
   - Pick a size on each shop card. Untick "Confirm each purchase" to buy
     with one click.

Use the 1000× / 5000× time buttons to let research and calendar time pass.
There is no save game yet; a career lasts as long as the server runs.

## Playing: a reactor startup

The server starts the plant at **hot standby**: no-load temperature and
pressure, all rods inserted, the turbine on turning gear. (Use
`--scenario full_power` to start at power instead, or the Plant Computer tab's
instructor station.) The tabs are panels of the control board, and each one
has its own annunciator. ACK/SIL/RST acknowledge, silence and reset alarms.

1. **Steam & Feed tab: condenser vacuum.** Circulating water pumps run at
   hot standby, but the condenser is at atmospheric pressure.
   - Start both vacuum (hogging) pumps and open SJAE steam. The SJAEs need
     main steam above 1.5 MPa.
   - Once below about 20 kPa, stop the hogging pumps.
   - With the Condenser Vacuum module in AUTO this happens by itself.
   - Steam dumps and turbine latching need vacuum, and the turbine trips
     above 30 kPa.
2. **Reactor tab: shutdown banks.** BANK SELECT starts on SA: hold ROD MOTION
   on OUT until SA reaches 228 steps, then do the same for SB. Control banks
   are interlocked until both are fully withdrawn (ROD WITHDRAWAL BLOCKED).
3. **Approach criticality.** Set BANK SELECT to MAN, which moves the control
   banks in overlap. Withdraw in steps while you watch the source range count
   rate and startup rate. SR SPEAKER lets you hear the counts.
4. **Block the source range at P-6.** Once the intermediate range is above
   1E-10 A, press **SR BLOCK**. If you don't, the SR high-flux trip at 1E5 cps
   scrams the reactor.
5. **Up the intermediate range to P-10.** Keep the startup rate under about
   1 dpm. Above P-10, press **IR BLOCK** and **PR LO BLOCK** before 25 %.
6. **Deaerator.** At low load it needs pegging steam, otherwise DEAERATION NOT
   EFFECTIVE stays lit. The Deaerator module does this in AUTO.
7. **Comms tab.** Call the TURBINE HALL for roll checks. In career this is
   required before latching the turbine.
8. **Turbine-Generator tab.**
   - The turbine needs about 4 % steam flow just to hold speed, so raise
     reactor power to about 8–12 % first.
   - Press TURBINE RESET, close the EXCITER FIELD BKR, pick an acceleration
     (S/M/F = 60/120/180 rpm/min) and press 1800.
   - Don't linger near the rotor critical speeds (about 1150 and 1650 rpm):
     vibration builds, the room shakes, and above 200 µm the turbine trips.
9. **Synchronize.**
   - At speed, call GRID DISPATCH for permission; in career, syncing without
     it costs a fine.
   - Trim speed so the synchroscope turns slowly FAST, then close the GEN
     BREAKER at 12 o'clock. The Auto-Synchronizer can do this in AUTO.
10. **Load the unit.** Grid dispatch phones in load requests: the phone rings,
    a toast appears and DEMAND flashes in the top bar.
    - Accept on the Grid tab to acknowledge, then raise turbine load
      yourself (governor target). With the Unit Load Controller in AUTO it
      takes the new target; with AGC on it accepts and follows by itself.
    - In career, revenue is better when net output stays on demand.

## What's simulated today (PWR)

- Four unit sizes (600 / 900 / 1150 / 1300 MWe) with scaled power, flow,
  volumes, pumps, house load and core layouts; sandbox size switch on the
  Equipment tab

- Point kinetics with 6 delayed-neutron groups (implicit, stable at any step)
- Doppler, moderator, boron, rod (S-curve), xenon-135 and samarium-149 reactivity
- 9-group decay heat fitted to Way–Wigner
- Fuel heat capacity, 4-node primary loop, RCP coastdown and natural circulation
- Pressurizer: insurge compression, heaters, sprays, PORV and safety valves
- Steam generators, turbine/generator, steam dumps, relief valves, feedwater heating
- Condenser (CW pumps, hogging vacuum pumps, SJAEs, hotwell), deaerator, MSIVs
- Turbine vibration with rotor critical speeds (and optional screen shake)
- Station electrical: startup transformer (grid) and unit auxiliary
  transformer (generator), 6.9 kV buses A/B with tie, 4.16 kV safety buses,
  two emergency diesels with load sequencer and fuel, 125 V DC with battery.
  Motor-start inrush can overload and lock out the startup transformer; bus B
  needs the generator or the A-B tie; dead buses stop their loads; loss of DC
  trips the reactor; the anti-motoring relay opens a motoring generator.
- Core exit thermocouples above all 193 fuel assemblies: radial power shape,
  cooler under inserted rods, quadrant tilt. The board shows core average,
  hottest, per-quadrant and per-rod-bank-area averages plus calculated fuel
  average; the Core Exit Thermocouple Map upgrade shows every assembly on
  the core map
- Plant messaging terminal on the reactor board, by installed equipment:
  vintage lamp message panel (lit caller windows, press to read/acknowledge),
  dated green-screen CRT terminal with side pushbuttons and function-key
  requests, or a modern HMI where you can type free-text requests ("calling
  for turbine runup"); anything not understood gets "MCR, please repeat"
- Grid dispatch station and plant telephones (turbine hall, grid, maintenance, chemistry)
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
