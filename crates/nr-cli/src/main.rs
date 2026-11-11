//! Headless NeuReactor runner.
//!
//! ```text
//! nr-cli scenario <steady|scram|load-follow|rcp-trip|xenon> [--seconds N] [--every S]
//! nr-cli bench [--seconds N]
//! ```
//! Scenarios print a CSV trace to stdout.

use nr_api::{Command, ModuleId, Origin};
use nr_reactors::Pwr;
use std::time::Instant;

const COLUMNS: &[&str] = &[
    "nuclear_power_pct",
    "thermal_power_mw",
    "gross_mwe",
    "tavg_c",
    "tref_c",
    "thot_c",
    "tcold_c",
    "rcs_pressure_mpa",
    "pzr_level_pct",
    "pzr_heaters_pct",
    "pzr_spray_pct",
    "charging_kg_s",
    "boron_ppm",
    "sg_pressure_mpa",
    "sg_level_pct",
    "steam_flow_kg_s",
    "feed_flow_kg_s",
    "turbine_valve_pct",
    "steam_dump_pct",
    "rho_xenon_pcm",
    "fuel_temp_c",
    "decay_heat_pct",
];

fn arg(args: &[String], name: &str, default: f64) -> f64 {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn cmd(p: &mut Pwr, c: Command) {
    if let Err(e) = p.command(&c, &Origin::Operator) {
        eprintln!("command {c:?} rejected: {e}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("scenario") => scenario(&args),
        Some("bench") => bench(arg(&args, "--seconds", 3600.0)),
        _ => {
            eprintln!("usage: nr-cli scenario <steady|scram|load-follow|rcp-trip|xenon> [--seconds N] [--every S]");
            eprintln!("       nr-cli bench [--seconds N]");
            std::process::exit(2);
        }
    }
}

fn scenario(args: &[String]) {
    let name = args.get(2).map(String::as_str).unwrap_or("steady");
    let seconds = arg(args, "--seconds", 1800.0);
    let every = arg(args, "--every", 10.0);
    let mut p = Pwr::full_power();

    // Scheduled actions: (time, command)
    let actions: Vec<(f64, Command)> = match name {
        "steady" => vec![],
        "scram" => vec![(60.0, Command::Scram)],
        "load-follow" => vec![
            (
                60.0,
                Command::SetSetpoint {
                    module: ModuleId::TurbineGovernor,
                    key: "ramp_mw_min".into(),
                    value: 50.0,
                },
            ),
            (
                60.0,
                Command::SetSetpoint {
                    module: ModuleId::TurbineGovernor,
                    key: "target_mwe".into(),
                    value: 575.0,
                },
            ),
            (
                1800.0,
                Command::SetSetpoint {
                    module: ModuleId::TurbineGovernor,
                    key: "target_mwe".into(),
                    value: 1150.0,
                },
            ),
        ],
        "rcp-trip" => vec![(
            60.0,
            Command::SetCoolantPump {
                index: 0,
                running: false,
            },
        )],
        "xenon" => vec![(60.0, Command::Scram)],
        other => {
            eprintln!("unknown scenario {other}");
            std::process::exit(2);
        }
    };

    println!("time_s,{},tripped", COLUMNS.join(","));
    let mut next_print = 0.0;
    let mut pending = actions.into_iter().peekable();
    while p.time <= seconds {
        while let Some((t, _)) = pending.peek() {
            if p.time >= *t {
                let (_, c) = pending.next().unwrap();
                cmd(&mut p, c);
            } else {
                break;
            }
        }
        if p.time >= next_print - 1e-9 {
            let truth = p.truth();
            let row: Vec<String> = COLUMNS
                .iter()
                .map(|c| format!("{:.3}", truth.get(*c).copied().unwrap_or(f64::NAN)))
                .collect();
            println!(
                "{:.1},{},{}",
                p.time,
                row.join(","),
                p.reactor_tripped as u8
            );
            next_print += every;
        }
        p.step();
    }
    for e in &p.events {
        eprintln!("[{:?}] {}", e.severity, e.text);
    }
}

fn bench(seconds: f64) {
    let mut p = Pwr::full_power();
    let start = Instant::now();
    p.advance(seconds);
    let wall = start.elapsed().as_secs_f64();
    println!(
        "simulated {seconds:.0} s in {wall:.3} s wall: {:.0}x real time ({:.1} µs/step)",
        seconds / wall,
        wall / (seconds / nr_reactors::pwr::DT) * 1e6
    );
}
