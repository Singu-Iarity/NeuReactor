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

fn sim_loop(world: Arc<Mutex<World>>) {
    let tick = Duration::from_millis(20);
    let mut last = Instant::now();
    let mut debt = 0.0;
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
        let mult = station.revenue_multiplier(plant);
        if let Some(c) = career.as_mut() {
            c.tick_with(elapsed, plant, mult);
        }
        if steps == 20_000 {
            debt = 0.0;
        }
        w.collect_events();
    }
}
