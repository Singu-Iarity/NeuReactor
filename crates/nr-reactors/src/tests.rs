//! Plant-level regression scenarios.

use crate::Pwr;
use nr_api::{Command, GameMode, ModuleId, ModuleMode, Origin, Scope};

fn op(p: &mut Pwr, c: Command) {
    p.command(&c, &Origin::Operator)
        .unwrap_or_else(|e| panic!("{c:?}: {e}"));
}

#[test]
fn full_power_is_steady() {
    let mut p = Pwr::full_power();
    assert!(p.rho.total.abs() < 1e-9);
    let b0 = p.boron_ppm;
    assert!((500.0..1500.0).contains(&b0), "critical boron {b0}");
    p.advance(600.0);
    let t = p.telemetry(1.0, false);
    let r = |k: &str| t.readings[k];
    assert!(!p.reactor_tripped, "trips: {:?}", p.trips);
    assert!(
        (r("nuclear_power_pct") - 100.0).abs() < 1.5,
        "power {}",
        r("nuclear_power_pct")
    );
    assert!((r("tavg_c") - 308.6).abs() < 1.0, "tavg {}", r("tavg_c"));
    assert!(
        (r("rcs_pressure_mpa") - 15.51).abs() < 0.1,
        "p {}",
        r("rcs_pressure_mpa")
    );
    assert!(
        (r("pzr_level_pct") - 55.0).abs() < 3.0,
        "pzr {}",
        r("pzr_level_pct")
    );
    assert!(
        (r("sg_level_pct") - 50.0).abs() < 5.0,
        "sg {}",
        r("sg_level_pct")
    );
    assert!(
        (r("gross_mwe") - 1150.0).abs() < 20.0,
        "mwe {}",
        r("gross_mwe")
    );
}

#[test]
fn scram_shuts_down_and_dumps_hold_no_load_tavg() {
    let mut p = Pwr::full_power();
    p.advance(10.0);
    op(&mut p, Command::Scram);
    p.advance(10.0);
    assert!(p.kinetics.n < 0.08, "n after 10 s = {}", p.kinetics.n);
    assert!(p.turbine_tripped && !p.breaker);
    p.advance(1200.0);
    let tavg = p.tavg();
    assert!((tavg - 291.7).abs() < 4.0, "tavg {tavg}");
    assert!(p.kinetics.n < 1e-3);
    let dh = p.decay.power();
    assert!((0.01..0.03).contains(&dh), "decay heat {dh}");
    assert!(p.pressure / 1e6 > 12.0, "pressure {}", p.pressure / 1e6);
}

#[test]
fn load_reduction_to_50_percent() {
    let mut p = Pwr::full_power();
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "ramp_mw_min".into(),
            value: 50.0,
        },
    );
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "target_mwe".into(),
            value: 575.0,
        },
    );
    p.advance(1800.0);
    assert!(!p.reactor_tripped, "trips: {:?}", p.trips);
    let n = p.kinetics.n;
    assert!((0.45..0.58).contains(&n), "power {n}");
    assert!((p.gross_mwe - 575.0).abs() < 25.0, "mwe {}", p.gross_mwe);
    assert!(
        (p.tavg() - p.tref()).abs() < 2.0,
        "tavg {} tref {}",
        p.tavg(),
        p.tref()
    );
}

#[test]
fn loss_of_all_rcps_trips_reactor() {
    let mut p = Pwr::full_power();
    for i in 0..4 {
        op(
            &mut p,
            Command::SetCoolantPump {
                index: i,
                running: false,
            },
        );
    }
    p.advance(30.0);
    assert!(p.reactor_tripped);
    assert!(
        p.trips.iter().any(|t| t.contains("low flow")),
        "{:?}",
        p.trips
    );
}

#[test]
fn manual_commands_blocked_while_in_auto() {
    let mut p = Pwr::full_power();
    let c = Command::MoveRodBank {
        bank: "D".into(),
        target: 100.0,
    };
    assert!(p.command(&c, &Origin::Operator).is_err());
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::RodControl,
            mode: ModuleMode::Manual,
        },
    );
    op(&mut p, c);
}

#[test]
fn career_plugin_cannot_drive_unowned_feedwater() {
    let mut p = Pwr::full_power();
    p.automation.game_mode = GameMode::Career;
    p.automation.owned = [ModuleId::RodControl, ModuleId::ExternalGateway]
        .into_iter()
        .collect();
    p.automation.modes.clear();
    let plugin = Origin::Plugin {
        id: "x".into(),
        scopes: [Scope::ControlWrite].into_iter().collect(),
    };
    assert!(p
        .command(&Command::SetFeedwater { fraction: 0.5 }, &plugin)
        .is_err());
    let rods = Command::MoveRodBank {
        bank: "D".into(),
        target: 150.0,
    };
    assert!(p.command(&rods, &plugin).is_err());
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::RodControl,
            mode: ModuleMode::External,
        },
    );
    assert!(p.command(&rods, &plugin).is_ok());
    // ...and now the operator must take it back to MANUAL before moving rods.
    assert!(p.command(&rods, &Origin::Operator).is_err());
}

#[test]
fn xenon_transient_after_trip_is_large() {
    let mut p = Pwr::full_power();
    op(&mut p, Command::Scram);
    p.advance(9.0 * 3600.0);
    assert!(p.rho.xenon < -0.035, "xenon {}", p.rho.xenon);
}

#[test]
fn boron_automation_restores_tavg_after_load_follow_xenon() {
    let mut p = Pwr::full_power();
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::BoronControl,
            mode: ModuleMode::Auto,
        },
    );
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "ramp_mw_min".into(),
            value: 50.0,
        },
    );
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "target_mwe".into(),
            value: 575.0,
        },
    );
    p.advance(1800.0);
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "target_mwe".into(),
            value: 1150.0,
        },
    );
    p.advance(3600.0);
    assert!(
        (p.tavg() - p.tref()).abs() < 1.5,
        "tavg {} tref {}",
        p.tavg(),
        p.tref()
    );
    assert!((p.gross_mwe - 1150.0).abs() < 20.0);
}
