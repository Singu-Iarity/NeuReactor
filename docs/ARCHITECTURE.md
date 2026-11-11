# Architecture

## Principles

1. **The simulation core is pure Rust with no UI dependencies.** Each front
   end is a thin client of the same command/telemetry API. Today that's the
   browser control room served by `nr-server`. Possible later front ends:
   a WebAssembly build for offline/hosted play, a native wgpu/egui client,
   or a C ABI so a C# front end (for example on the BRUTAL framework) can
   embed the core.
2. **Deterministic, fixed-step physics.** `DT = 50 ms`, stiff parts
   integrated implicitly. The host advances the plant `time_scale × wall`
   seconds per tick, so time acceleration up to ×1000 costs only CPU. The
   current PWR runs at about 13,000× real time on one core.
3. **Reactor types are assembled from shared components.** `nr-physics`
   holds the phenomena (kinetics, poisons, decay heat, rods, sensors).
   `nr-reactors` wires them into a plant.
4. **Instruments are not the truth.** Operators and plugins see sensor
   readings (lag, noise, drift, faults). True state is only exposed through
   the sandbox-only `truth` endpoint.
5. **Every actuator has exactly one owning automation module**
   (`nr_api::modules::CATALOG`). A module's mode decides who may move its
   actuators: MANUAL means the operator, AUTO means the built-in controller,
   EXTERNAL means an approved plugin.

## Equipment: computers, panel packages, automation modules

A unit's I&C is built up from three kinds of equipment. Sandbox lets you
toggle each item; career mode will buy them.

| Layer | Examples | What it does |
|---|---|---|
| **Computer** (`nr_api::upgrades`, kind `computer`) | Plant Process Computer, Turbine Control Computer | Runs automation software. Cannot see or drive any panel on its own. |
| **Panel package** (`kind: panel_package`) | Generator Sync Interface, Turbine Control Panel Package, Digital NIS | Replaces one board area's original analog equipment with modern controls/displays and wires it to its computer. Covers specific board sections (`"turbine.sync"`), so only those sections change look. |
| **Automation module** (`nr_api::modules`) | Auto-Synchronizer, Turbine Governor, Feedwater Level | Software for one subsystem. Runs in AUTO/EXT only if installed **and** its `requires_upgrades` (its computer + its panel package) are installed. |

Hardware can also change behaviour: with the Turbine Control Computer *and*
the Generator Sync Interface, the sync-check relay is permanent, so bad
breaker closes are always blocked. Removing a computer or package drops
every module that depends on it to MANUAL. Telemetry reports
`sections: {"turbine.sync": "digital", ...}`, and the UI themes each
`data-section` independently.

## Career layer (`nr-game`)

`Career` owns the calendar, money, research and the purchased configuration
(`nr_api::equipment::Equipment`: component era/tier per slot, controller
variant per module, fitted conversion kits; plus installed modules and I&C
hardware). Purchases are quoted (price + newly required kits − refund) and
pushed into the plant with `Pwr::install`, which drops any automation that
can no longer run to MANUAL. The server ticks the career with plant time.

## Data flow

```
 browser UI ─┐                       ┌─> nr-api::authorize (game mode, ownership, scopes)
 plugins  ───┼─ HTTP /api/v1 ─> nr-server ─> Pwr::command ─> plant-level interlocks
 nr-cli   ───┘                       └─> sim thread: Pwr::step()
                                              read_instruments → run_automation →
                                              run_protection → rods → neutronics →
                                              primary → secondary
```

## Access policy (career-mode integrity)

Plugins get capability scopes: `telemetry_read`, `truth_read`,
`control_write`, `sim_control`, `scenario_write`, `economy_read` and
`ui_panel`. The operator approves each plugin in the UI.

- **Sandbox:** a plugin may do anything its scopes allow.
- **Career:** `truth_read` and `scenario_write` are disabled. A plugin may
  move an actuator only if the player **owns** its automation module, owns
  the **External Control Gateway**, and has put that module in **EXTERNAL**
  mode. A plugin can replace or improve automation you have bought. It can
  never automate a subsystem you haven't. Only the operator can change
  module modes or reset trips.

The policy lives in `crates/nr-api/src/policy.rs` and has unit tests.
