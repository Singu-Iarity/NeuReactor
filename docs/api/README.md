# NeuReactor plugin API (v1)

The server listens on `127.0.0.1:8642` (`--port` to change). All responses are
JSON. Authenticate with `Authorization: Bearer <token>` (or `?token=`).

## Connecting a plugin

1. `POST /api/v1/plugins/register` with `{"name": "My tool", "scopes": ["telemetry_read"]}`.
   You get `{"id", "token", "status": "pending"}`.
2. The operator approves it in the control room (Plugins panel).
3. Poll `GET /api/v1/plugins/me` until `approved` is `true`, then use the token.

## Scopes

| Scope | Grants | Career mode |
|---|---|---|
| `telemetry_read` | `/telemetry`, `/events` | yes |
| `truth_read` | `/truth` (true physics state) | **no** |
| `control_write` | actuator and automation commands | only for owned modules in EXTERNAL mode |
| `sim_control` | `set_time_scale`, `set_paused` | yes |
| `scenario_write` | fault injection, training mode | **no** |
| `economy_read` | finances, market (future) | yes |
| `ui_panel` | custom control room panels (future) | yes |

## Endpoints

| Method & path | Scope | Description |
|---|---|---|
| `GET /api/v1` | none | Version, endpoints, module catalog |
| `GET /api/v1/telemetry` | telemetry_read | Instrument readings, rods, alarms, trips, module states |
| `GET /api/v1/truth` | truth_read | True values incl. reactivity components, fuel temp, decay heat |
| `GET /api/v1/events?since=SEQ` | telemetry_read | Event log after sequence number |
| `POST /api/v1/command` | depends | Body is a `Command` (below) |

## Commands

JSON objects tagged by `type`:

```json
{"type": "move_rod_bank", "bank": "D", "target": 180}
{"type": "rod_motion", "selection": "manual", "direction": "out"}   // selection: manual | SA | SB | A | B | C | D; direction: in | hold | out
{"type": "set_boration", "mode": "dilute"}            // off | borate | dilute
{"type": "set_pressurizer_heaters", "fraction": 0.5}
{"type": "set_pressurizer_spray", "fraction": 0.0}
{"type": "set_charging", "kg_s": 7.5}
{"type": "set_letdown", "kg_s": 7.5}
{"type": "set_coolant_pump", "index": 0, "running": true}
{"type": "set_feedwater", "fraction": 0.77}
{"type": "set_steam_dump", "fraction": 0.0}
{"type": "set_turbine_valve", "fraction": 0.85}
{"type": "set_generator_breaker", "closed": true}        // sync-checked when the room is digital or generator_sync is AUTO
{"type": "set_field_breaker", "closed": true}
{"type": "set_excitation", "fraction": 0.5}
{"type": "block_nis", "channel": "source_range", "blocked": true}   // source_range (needs P-6) | intermediate_range | power_range_low (need P-10)
{"type": "scram"}
{"type": "reset_trip"}
{"type": "trip_turbine"}
{"type": "reset_turbine"}
{"type": "set_module_mode", "module": "rod_control", "mode": "auto"}   // manual | auto | external
{"type": "set_setpoint", "module": "turbine_governor", "key": "target_mwe", "value": 900}
{"type": "set_time_scale", "factor": 60}
{"type": "set_paused", "paused": true}
{"type": "inject_sensor_fault", "tag": "tavg_c", "fault": "stuck"}   // none | stuck | fail_low | fail_high
{"type": "set_training_mode", "enabled": true}
{"type": "repair_generator"}                                  // sandbox only
{"type": "set_upgrade_installed", "upgrade": "generator_sync_interface", "installed": true}   // sandbox only
{"type": "set_module_installed", "module": "generator_sync", "installed": true}               // sandbox only
{"type": "set_control_room", "generation": "digital"}         // preset: digital = install everything, analog = original plant (sandbox only)
{"type": "load_scenario", "name": "hot_standby"}              // hot_standby | full_power (sandbox only)
```

An actuator whose module is in `auto` rejects manual commands. Switch the
module to `manual` (operator) or `external` (plugin) first.

## Module ids

`rod_control`, `boron_control`, `pressurizer_pressure`, `pressurizer_level`,
`rcp_sequencer`, `feedwater_level`, `deaerator`, `condenser_vacuum`,
`steam_dump`, `turbine_governor`, `generator_sync`, `excitation`, `diesel_sequencer`,
`aux_feed_auto_start`, `plant_master`, `external_gateway`. Modules marked
`implemented: false` in `GET /api/v1` are in the catalog but their plant
systems aren't simulated yet.

## Telemetry tags

`sr_cps`, `sr_sur_dpm`, `ir_amps`, `ir_sur_dpm`, `pr_n41_pct` … `pr_n44_pct`,
`nuclear_power_pct`, `thermal_power_mw`, `startup_rate_dpm`, `reactivity_pcm`,
`tavg_c`, `tref_c`, `thot_c`, `tcold_c`, `delta_t_pct`, `rcs_pressure_mpa`,
`pzr_level_pct`, `pzr_level_program_pct`, `rcs_flow_pct`, `boron_ppm`,
`pzr_heaters_pct`, `pzr_spray_pct`, `charging_kg_s`, `letdown_kg_s`,
`sg_pressure_mpa`, `sg_level_pct`, `steam_flow_kg_s`, `feed_flow_kg_s`,
`feed_demand_pct`, `feed_temp_c`, `turbine_valve_pct`, `steam_dump_pct`,
`gross_mwe`, `net_mwe`, `turbine_speed_rpm`, `gen_freq_hz`, `grid_freq_hz`,
`sync_phase_deg`, `sync_slip_hz`, `gen_voltage_kv`, `grid_voltage_kv`, `excitation_pct`,
`generator_health_pct`.

Besides `readings`, telemetry carries discrete status: `nis` (blocks, SR high
voltage, P-6/P-10), `generator_breaker`, `field_breaker`, `generator_lockout`,
`sync_check_active`, `sync_blocked`, `sections` (board section → analog/digital),
`upgrades` (installed I&C hardware), `scenario`, `rod_motion`,
and `alarms` (each with the control room `panel` it belongs to).

See `examples/plugins/telemetry_logger.py` for a complete example.
