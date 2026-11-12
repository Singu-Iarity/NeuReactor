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
    assert!(p.turbine_tripped);
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
    p.advance(5400.0);
    assert!(
        (p.tavg() - p.tref()).abs() < 1.5,
        "tavg {} tref {}",
        p.tavg(),
        p.tref()
    );
    assert!((p.gross_mwe - 1150.0).abs() < 20.0);
}

#[test]
fn house_load_comes_from_running_equipment() {
    let p = Pwr::full_power();
    let h = p.house_load();
    assert!((30.0..80.0).contains(&h), "full power house load {h}");
    let s = Pwr::hot_standby();
    assert!(
        s.elec.import_mw > 10.0,
        "shutdown plant imports {}",
        s.elec.import_mw
    );
}

#[test]
fn loss_of_offsite_power_with_diesel_sequencer() {
    let mut p = Pwr::full_power();
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::DieselSequencer,
            mode: ModuleMode::Auto,
        },
    );
    op(&mut p, Command::SetOffsitePower { available: false });
    p.advance(60.0);
    assert!(!p.tg.breaker, "generator must be islanded");
    assert!(
        p.reactor_tripped,
        "loss of forced flow must trip the reactor"
    );
    assert!(p.rcp_running.iter().all(|r| !r));
    assert!(
        p.elec.buses.safety1 && p.elec.buses.safety2,
        "diesels pick up the safety buses"
    );
    assert!(p.elec.buses.dc);
}

#[test]
fn loss_of_offsite_power_without_diesels_drains_battery() {
    let mut p = Pwr::full_power();
    op(&mut p, Command::SetOffsitePower { available: false });
    p.advance(60.0);
    assert!(!p.elec.buses.safety1 && !p.elec.buses.safety2);
    assert!(p.elec.battery_pct < 100.0);
    assert!(p.alarms_for_test().iter().any(|a| a == "batt_discharge"));
}

#[test]
fn core_exit_thermocouples_track_power_and_rods() {
    let p = Pwr::full_power();
    let t = p.core_exit_temps();
    assert_eq!(t.len(), 193);
    let avg = t.iter().sum::<f64>() / t.len() as f64;
    assert!((avg - p.t_hot).abs() < 1.0, "avg {avg} vs thot {}", p.t_hot);
    let max = t.iter().cloned().fold(f64::MIN, f64::max);
    let min = t.iter().cloned().fold(f64::MAX, f64::min);
    eprintln!(
        "full power CET avg {avg:.1} min {min:.1} max {max:.1} thot {:.1} tcold {:.1}",
        p.t_hot, p.t_cold
    );
    assert!(max > avg + 5.0 && min < avg - 5.0);
    let s = Pwr::hot_standby();
    let ts = s.core_exit_temps();
    let spread =
        ts.iter().cloned().fold(f64::MIN, f64::max) - ts.iter().cloned().fold(f64::MAX, f64::min);
    assert!(spread < 3.0, "no power, flat core exit temps ({spread})");
}

#[test]
fn low_pzr_level_isolates_letdown_and_auto_recovers() {
    let mut p = Pwr::full_power();
    p.rcs_mass -= 20_000.0;
    p.advance(30.0);
    assert!(p.letdown_isolated, "letdown isolates at low level");
    assert_eq!(p.letdown, 0.0);
    assert_eq!(p.heaters, 0.0, "heaters cut off");
    // Manual re-open is refused while level is still low.
    assert!(p
        .command(&Command::SetLetdown { kg_s: 7.5 }, &Origin::Operator)
        .is_err());
    // Level control in AUTO refills and re-establishes letdown.
    p.advance(2400.0);
    assert!(!p.letdown_isolated);
    assert!(p.letdown > 5.0);
    assert!(
        (p.pzr_level() - 54.0).abs() < 6.0,
        "level {}",
        p.pzr_level()
    );
    assert!(!p.reactor_tripped);
}

#[test]
fn low_pzr_level_in_manual_needs_operator_charging() {
    let mut p = Pwr::full_power();
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::PressurizerLevel,
            mode: ModuleMode::Manual,
        },
    );
    p.rcs_mass -= 20_000.0;
    p.advance(60.0);
    assert!(p.letdown_isolated);
    op(&mut p, Command::SetCharging { kg_s: 20.0 });
    p.advance(1500.0);
    assert!(p.pzr_level() > 17.0, "level {}", p.pzr_level());
    op(&mut p, Command::SetLetdown { kg_s: 7.5 });
    assert!(!p.letdown_isolated);
}

#[test]
fn every_unit_size_is_steady_at_full_power_and_standby() {
    use nr_api::UnitSize;
    for size in UnitSize::ALL {
        let mut p = Pwr::full_power_sized(size);
        p.advance(900.0);
        eprintln!(
            "{:?}: power {:.1}% gross {:.0} MWe (rated {:.0}) house {:.1} tavg {:.1} pzr {:.1}% P {:.2} MPa trip={}",
            size,
            p.thermal_fraction() * 100.0,
            p.gross_mwe,
            size.mwe(),
            p.house_load(),
            p.tavg(),
            p.pzr_level(),
            p.pressure / 1e6,
            p.reactor_tripped
        );
        assert!(!p.reactor_tripped, "{size:?} tripped: {:?}", p.trips);
        assert!((p.thermal_fraction() - 1.0).abs() < 0.03, "{size:?} power");
        assert!(
            (p.gross_mwe / size.mwe() - 1.0).abs() < 0.04,
            "{size:?} output {}",
            p.gross_mwe
        );
        assert!((p.tavg() - 308.6).abs() < 2.0, "{size:?} tavg");
        assert!((p.pzr_level() - 55.0).abs() < 8.0, "{size:?} pzr level");
        let mut s = Pwr::hot_standby_sized(size);
        s.advance(600.0);
        assert!(!s.reactor_tripped, "{size:?} hot standby tripped");
        assert!(
            (s.tavg() - 291.7).abs() < 2.0,
            "{size:?} standby tavg {}",
            s.tavg()
        );
    }
}
