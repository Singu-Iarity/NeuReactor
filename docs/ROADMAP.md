# Roadmap

- [x] **M0 – Scaffold:** Cargo workspace, crates, local server, web UI, CI, docs, `nr-api` (commands, telemetry, scopes, policy, module catalog)
- [~] **M1 – Physics core:** point kinetics, poisons, decay heat, rods, primary/secondary loops, RPS, CLI scenarios *(done)*; nodal core, IF97, fuel pin model, ECCS *(next)*
- [~] **M2 – PWR sandbox:** browser control room, per-subsystem automation modules, plugin API *(done)*; save/load, startup-from-cold scenario, WebSocket push, plugin UI panels *(next)*
- [ ] **M3 – Malfunctions & instruments:** component wear, random and wear-driven failures, maintenance actions, analog vs digital instrument sets
- [ ] **M4 – Career / economy:** grid market (time-of-day prices, contracts), finances, buying and installing automation modules, spare parts inventory with lead times, outages and refuelling, regulator inspections, digital I&C skin
- [ ] **M5 – More reactors:** BWR, RBMK-1000 to RBMK-1500 generation upgrade (positive void coefficient, graphite, ORM, local automatic regulators, spatial scram effect), multi-unit sites
- [ ] **M6 – Fast breeder:** sodium-cooled core, breeding blanket, product sales instead of only power; WebAssembly offline build and optional native front end
