//! NeuReactor local server.
//!
//! Runs the plant simulation natively and serves, on 127.0.0.1 only:
//! * the control room web UI at `/`;
//! * the versioned JSON API at `/api/v1/...` used by the UI and by plugins.
//!
//! ```text
//! neureactor [--port 8642] [--scenario hot_standby|full_power]
//! ```

mod api;
mod saves;
mod world;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use world::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok())
        .unwrap_or(8642);

    let scenario = args
        .iter()
        .position(|a| a == "--scenario")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("hot_standby");
    let world = Arc::new(Mutex::new(World::new(scenario)));

    // Simulation thread: fixed physics step, paced against wall clock.
    {
        let world = Arc::clone(&world);
        thread::spawn(move || sim_loop(world));
    }

    let addr = format!("127.0.0.1:{port}");
    let server = tiny_http::Server::http(&addr).unwrap_or_else(|e| {
        eprintln!("cannot listen on {addr}: {e}");
        std::process::exit(1);
    });
    println!("NeuReactor control room: http://{addr}/");
    println!("Plugin API:              http://{addr}/api/v1  (see docs/api/README.md)");

    let server = Arc::new(server);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let server = Arc::clone(&server);
            let world = Arc::clone(&world);
            thread::spawn(move || {
                for req in server.incoming_requests() {
                    api::handle(req, &world);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
}

/// Auto time warp: speed up while the unit is steady on its dispatch target
/// with nothing needing attention; slow back down the moment a new grid
/// request arrives, output leaves the target band, or an alarm comes in.
fn auto_warp(w: &mut World) {
    if !w.auto_warp {
        if w.warping {
            w.warping = false;
            w.time_scale = w.warp_return;
        }
        w.warp_reason.clear();
        w.warp_stage.clear();
        return;
    }
    let plant = &w.plant;
    let st = &w.station;
    let rated_net = plant.p.gross_mwe - plant.p.house_load_mwe;
    let net = plant.gross_mwe - plant.house_load();
    let on_demand = st.reached && (net - st.demand_mwe).abs() <= 0.03 * rated_net;
    // Stage: hold (operator needed), ramp (load target set, output moving),
    // warp (steady on the commitment).
    let (stage, reason): (&str, String) = if st.pending.is_some() {
        ("hold", "new grid request".into())
    } else if !plant.tg.breaker {
        ("hold", "unit off-line".into())
    } else if st.demand_mwe <= 0.0 {
        ("hold", "no dispatch commitment".into())
    } else if plant.reactor_tripped {
        ("hold", "reactor tripped".into())
    } else if let Some(a) = plant.serious_alarm() {
        ("hold", format!("alarm: {a}"))
    } else if on_demand {
        ("warp", String::new())
    } else {
        // Ramp only once the load target matches the commitment.
        let wanted = (st.demand_mwe + plant.house_load()).min(plant.load_limit_mwe());
        let master =
            plant.automation.mode(nr_api::ModuleId::PlantMaster) == nr_api::ModuleMode::Auto;
        let module = if master {
            nr_api::ModuleId::PlantMaster
        } else {
            nr_api::ModuleId::TurbineGovernor
        };
        let governor_auto =
            plant.automation.mode(nr_api::ModuleId::TurbineGovernor) == nr_api::ModuleMode::Auto;
        let target = plant
            .automation
            .setpoint(module, "target_mwe")
            .unwrap_or(0.0);
        if !governor_auto {
            ("hold", "turbine governor not in AUTO".into())
        } else if (target - wanted).abs() > (0.02 * rated_net).max(10.0) {
            (
                "hold",
                format!("set turbine load target to {wanted:.0} MWe gross"),
            )
        } else {
            ("ramp", String::new())
        }
    };
    let scale = match stage {
        "warp" => Some(w.warp_factor),
        "ramp" => Some(w.ramp_factor.min(w.warp_factor)),
        _ => None,
    };
    match scale {
        Some(f) => {
            if !w.warping {
                w.warp_return = w.time_scale.min(f);
                w.warping = true;
            }
            if w.warp_stage != stage {
                w.log(nr_reactors::PlantEvent::info(&format!(
                    "Auto time warp: {stage} {f}x"
                )));
            }
            w.time_scale = f;
            w.warp_reason.clear();
        }
        None => {
            // Something needs the operator: drop to real time (1x), not
            // to whatever speed was set before warping. A new grid
            // request always does, even while already holding fast.
            let new_request = w.station.pending.as_ref().map(|r| r.id);
            let fresh_request = new_request.is_some() && new_request != w.warp_seen_request;
            if w.warping || fresh_request {
                w.warping = false;
                w.time_scale = 1.0;
                w.log(nr_reactors::PlantEvent::warn(&format!(
                    "Auto time warp: slowed to 1x: {reason}"
                )));
            }
            w.warp_seen_request = new_request.or(w.warp_seen_request);
            w.warp_reason = reason;
        }
    }
    w.warp_stage = stage.to_string();
}

fn sim_loop(world: Arc<Mutex<World>>) {
    let tick = Duration::from_millis(20);
    let mut last = Instant::now();
    let mut debt = 0.0;
    let mut last_autosave = Instant::now();
    loop {
        thread::sleep(tick);
        let now = Instant::now();
        let wall = (now - last).as_secs_f64();
        last = now;
        let mut w = world.lock().unwrap();
        if w.paused {
            debt = 0.0;
            continue;
        }
        debt += wall * w.time_scale;
        let dt = nr_reactors::pwr::DT;
        // Cap work per tick so a huge time scale cannot starve the API.
        let mut steps = 0;
        let t0 = w.plant.time;
        while debt >= dt && steps < 20_000 {
            w.plant.step();
            debt -= dt;
            steps += 1;
        }
        let World {
            plant,
            career,
            station,
            ..
        } = &mut *w;
        let elapsed = plant.time - t0;
        // Calendar for system demand: the career date, or Mon 15 Jan 06:00
        // plus game time in sandbox.
        station.calendar_s = match career.as_ref() {
            Some(c) => c.elapsed_days * 86_400.0,
            None => 14.0 * 86_400.0 + 6.0 * 3600.0 + plant.time,
        };
        if elapsed > 0.0 {
            let events = station.tick(plant, elapsed);
            for e in events {
                if e.text.contains("WITHOUT grid permission") {
                    if let Some(c) = career.as_mut() {
                        c.fine(
                            250_000.0,
                            "synchronised to the grid without dispatch permission",
                        );
                    }
                }
                plant.events.push(e);
            }
            for (amount, why) in std::mem::take(&mut station.credits) {
                plant.events.push(nr_reactors::PlantEvent::info(&format!(
                    "Grid premium: {why}"
                )));
                if let Some(c) = career.as_mut() {
                    c.credit(amount, &why);
                }
            }
            for (amount, why) in std::mem::take(&mut station.fines) {
                plant.events.push(nr_reactors::PlantEvent::warn(&format!(
                    "Grid penalty: {why}"
                )));
                if let Some(c) = career.as_mut() {
                    c.fine(amount, &why);
                }
            }
        }
        // AGC: the unit load controller follows grid dispatch (needs the
        // plant master in AUTO and the grid dispatch link).
        if station.agc && !plant.agc_available() {
            station.agc = false;
        }
        if station.agc && station.demand_mwe > 0.0 {
            let gross = station.demand_mwe + plant.house_load();
            let module = nr_api::ModuleId::PlantMaster;
            let cur = plant
                .automation
                .setpoint(module, "target_mwe")
                .unwrap_or(0.0);
            if (cur - gross).abs() > 0.5 {
                let _ = plant.command(
                    &nr_api::Command::SetSetpoint {
                        module,
                        key: "target_mwe".into(),
                        value: gross,
                    },
                    &nr_api::Origin::Operator,
                );
            }
        }
        if elapsed > 0.0 {
            let (price, om) = career
                .as_ref()
                .map(|c| (c.price_mwh, c.om_per_day))
                .unwrap_or((45.0, 120_000.0));
            station.account(plant, price, om, elapsed);
        }
        let mult = station.revenue_multiplier(plant);
        if let Some(c) = career.as_mut() {
            c.tick_with(elapsed, plant, mult);
        }
        if steps == 20_000 {
            debt = 0.0;
        }
        auto_warp(&mut w);
        // Autosave every five minutes of real time.
        if last_autosave.elapsed() > Duration::from_secs(300) {
            last_autosave = Instant::now();
            if let Err(e) = saves::save(&w, saves::AUTOSAVE) {
                eprintln!("autosave failed: {e}");
            }
        }
        w.collect_events();
    }
}
