//! Startup, nuclear instrumentation and generator synchronisation scenarios.

use crate::Pwr;
use nr_api::{
    Command, ControlRoomGeneration, ModuleId, ModuleMode, NisBlock, Origin, RodDirection, UpgradeId,
};

fn op(p: &mut Pwr, c: Command) {
    p.command(&c, &Origin::Operator)
        .unwrap_or_else(|e| panic!("{c:?}: {e}"));
}

fn lever(p: &mut Pwr, sel: &str, d: RodDirection) {
    op(
        p,
        Command::RodMotion {
            selection: sel.into(),
            direction: d,
        },
    );
}

fn reading(p: &Pwr, tag: &str) -> f64 {
    p.telemetry(1.0, false).readings[tag]
}

/// Operator-style rod withdrawal: pull while the startup rate is low,
/// hold when it gets brisk, until `stop` returns true.
fn approach(p: &mut Pwr, max_s: f64, mut stop: impl FnMut(&Pwr) -> bool) {
    let mut t = 0.0;
    while t < max_s {
        if stop(p) {
            lever(p, "manual", RodDirection::Hold);
            return;
        }
        let sur = reading(p, "startup_rate_dpm");
        let d = if sur < 0.4 {
            RodDirection::Out
        } else if sur > 1.0 {
            RodDirection::In
        } else {
            RodDirection::Hold
        };
        lever(p, "manual", d);
        p.advance(1.0);
        t += 1.0;
        assert!(
            !p.reactor_tripped,
            "tripped during approach at t={t}: {:?}",
            p.trips
        );
    }
    panic!("approach did not finish in {max_s} s");
}

/// Hot standby -> criticality -> P-6 -> P-10 -> synchronised at low load.
fn startup_to_sync() -> Pwr {
    let mut p = Pwr::hot_standby();
    p.advance(30.0);
    let sr = reading(&p, "sr_cps");
    assert!((3.0..200.0).contains(&sr), "shutdown SR counts {sr}");
    assert!(reading(&p, "ir_amps") < 2e-11);
    assert!(!p.reactor_tripped);

    // Shutdown banks out.
    for bank in ["SA", "SB"] {
        lever(&mut p, bank, RodDirection::Out);
        p.advance(300.0);
        lever(&mut p, bank, RodDirection::Hold);
    }
    assert_eq!(p.banks[1].position, 228.0);

    // Control banks to criticality and up through the source range.
    approach(&mut p, 3600.0, |p| p.telemetry(1.0, false).nis.p6);
    op(
        &mut p,
        Command::BlockNis {
            channel: NisBlock::SourceRange,
            blocked: true,
        },
    );
    assert_eq!(
        reading(&p, "sr_cps"),
        0.0,
        "SR high voltage must be off once blocked"
    );

    // Intermediate range up to P-10.
    approach(&mut p, 3600.0, |p| p.telemetry(1.0, false).nis.p10);
    op(
        &mut p,
        Command::BlockNis {
            channel: NisBlock::IntermediateRange,
            blocked: true,
        },
    );
    op(
        &mut p,
        Command::BlockNis {
            channel: NisBlock::PowerRangeLow,
            blocked: true,
        },
    );
    approach(&mut p, 600.0, |p| p.kinetics.n > 0.12);
    p.advance(120.0);
    assert!(!p.reactor_tripped, "{:?}", p.trips);

    // Roll the turbine and synchronise automatically.
    op(&mut p, Command::ResetTurbine);
    op(&mut p, Command::SetFieldBreaker { closed: true });
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "speed_rpm".into(),
            value: 1800.0,
        },
    );
    p.advance(420.0);
    assert!(
        (reading(&p, "turbine_speed_rpm") - 1800.0).abs() < 10.0,
        "rpm {}",
        reading(&p, "turbine_speed_rpm")
    );
    assert!((reading(&p, "gen_voltage_kv") - 22.0).abs() < 0.5);
    p
}

#[test]
fn hot_standby_startup_and_auto_sync() {
    let mut p = startup_to_sync();
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::GeneratorSync,
            mode: ModuleMode::Auto,
        },
    );
    p.advance(180.0);
    assert!(
        p.tg.breaker,
        "auto-synchronizer did not close: phase {} slip {}",
        p.tg.phase,
        p.tg.slip_hz()
    );
    assert_eq!(p.tg.health, 100.0);
    // Pick up load on the governor, rods in auto.
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::RodControl,
            mode: ModuleMode::Auto,
        },
    );
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "target_mwe".into(),
            value: 100.0,
        },
    );
    p.advance(600.0);
    assert!(!p.reactor_tripped, "{:?}", p.trips);
    assert!(p.tg.breaker);
    assert!((p.gross_mwe - 100.0).abs() < 15.0, "mwe {}", p.gross_mwe);
}

fn wait_for_phase(p: &mut Pwr, lo: f64, hi: f64) {
    for _ in 0..20_000 {
        if (lo..=hi).contains(&p.tg.phase) {
            return;
        }
        p.step();
    }
    panic!("phase never reached {lo}..{hi}");
}

#[test]
fn analog_out_of_phase_close_damages_and_trips() {
    let mut p = startup_to_sync();
    // Original analog synchroscope panel: no sync-check relay.
    op(
        &mut p,
        Command::SetUpgradeInstalled {
            upgrade: UpgradeId::GeneratorSyncInterface,
            installed: false,
        },
    );
    assert!(!p.sync_check_active());
    // Make the synchroscope turn so the phase sweeps.
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "speed_rpm".into(),
            value: 1803.0,
        },
    );
    p.advance(30.0);
    wait_for_phase(&mut p, 120.0, 180.0);
    op(&mut p, Command::SetGeneratorBreaker { closed: true });
    assert!(!p.tg.breaker);
    assert!(p.turbine_tripped);
    assert!(p.tg.health < 60.0, "health {}", p.tg.health);
}

#[test]
fn sync_check_blocks_and_lamp_flashes() {
    let mut p = startup_to_sync();
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::GeneratorSync,
            mode: ModuleMode::Auto,
        },
    );
    // Hold off the auto close by keeping speed off target: governor manual.
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::TurbineGovernor,
            mode: ModuleMode::Manual,
        },
    );
    wait_for_phase(&mut p, 60.0, 170.0);
    let r = p.command(
        &Command::SetGeneratorBreaker { closed: true },
        &Origin::Operator,
    );
    assert!(r.is_err());
    assert!(!p.tg.breaker);
    assert!(p.telemetry(1.0, false).sync_blocked);
    assert_eq!(p.tg.health, 100.0);
    assert!(p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "sync_blocked"));
}

#[test]
fn generator_sync_interface_has_permanent_sync_check() {
    let mut p = startup_to_sync();
    let up = |p: &mut Pwr, u, i| {
        op(
            p,
            Command::SetUpgradeInstalled {
                upgrade: u,
                installed: i,
            },
        )
    };
    // Fully equipped sandbox: computer + sync package => sync-check.
    assert!(p.sync_check_active());
    // Without the sync panel package the computer cannot reach the breaker.
    up(&mut p, UpgradeId::GeneratorSyncInterface, false);
    assert!(!p.sync_check_active());
    // Package alone (computer removed): still no sync-check.
    up(&mut p, UpgradeId::TurbineControlComputer, false);
    up(&mut p, UpgradeId::GeneratorSyncInterface, true);
    assert!(!p.sync_check_active());
    // Computer + package again.
    up(&mut p, UpgradeId::TurbineControlComputer, true);
    assert!(p.sync_check_active());
    // Strip the turbine control panel package only: that section reverts to
    // analog while the sync section stays modern.
    up(&mut p, UpgradeId::TurbinePanelInterface, false);
    up(&mut p, UpgradeId::DigitalNis, false);
    let t = p.telemetry(1.0, false);
    assert_eq!(t.sections["turbine.sync"], ControlRoomGeneration::Digital);
    assert_eq!(t.sections["turbine.control"], ControlRoomGeneration::Analog);
    assert_eq!(t.sections["reactor.nis"], ControlRoomGeneration::Analog);
    // Governor auto needs its panel package back for the rest of the test.
    up(&mut p, UpgradeId::TurbinePanelInterface, true);
    for m in [ModuleId::TurbineGovernor, ModuleId::Excitation] {
        op(
            &mut p,
            Command::SetModuleMode {
                module: m,
                mode: ModuleMode::Auto,
            },
        );
    }
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "speed_rpm".into(),
            value: 1803.0,
        },
    );
    p.advance(30.0);
    wait_for_phase(&mut p, 90.0, 180.0);
    assert!(p
        .command(
            &Command::SetGeneratorBreaker { closed: true },
            &Origin::Operator
        )
        .is_err());
    assert_eq!(p.tg.health, 100.0);
}

#[test]
fn unblocked_source_range_trips_on_power_rise() {
    let mut p = Pwr::hot_standby();
    for bank in ["SA", "SB"] {
        lever(&mut p, bank, RodDirection::Out);
        p.advance(300.0);
    }
    lever(&mut p, "manual", RodDirection::Out);
    p.advance(3000.0);
    assert!(p.reactor_tripped);
    assert_eq!(
        p.trips.first().map(String::as_str),
        Some("Source range high flux")
    );
}

#[test]
fn source_range_block_needs_p6() {
    let mut p = Pwr::hot_standby();
    let c = Command::BlockNis {
        channel: NisBlock::SourceRange,
        blocked: true,
    };
    assert!(p.command(&c, &Origin::Operator).is_err());
    let c = Command::BlockNis {
        channel: NisBlock::IntermediateRange,
        blocked: true,
    };
    assert!(p.command(&c, &Origin::Operator).is_err());
}

#[test]
fn generator_repair_clears_lockout() {
    let mut p = Pwr::full_power();
    p.tg.health = 30.0;
    p.tg.lockout = true;
    op(&mut p, Command::RepairGenerator);
    assert!(!p.tg.lockout && p.tg.health == 100.0);
}

#[test]
fn removing_modules_and_hardware_in_sandbox() {
    let mut p = Pwr::full_power();
    // Computer without the feedwater panel package cannot run feedwater auto.
    op(
        &mut p,
        Command::SetUpgradeInstalled {
            upgrade: UpgradeId::DigitalFeedwater,
            installed: false,
        },
    );
    assert_eq!(
        p.automation.mode(ModuleId::FeedwaterLevel),
        ModuleMode::Manual
    );
    let fw = Command::SetModuleMode {
        module: ModuleId::FeedwaterLevel,
        mode: ModuleMode::Auto,
    };
    assert!(p.command(&fw, &Origin::Operator).is_err());
    op(
        &mut p,
        Command::SetUpgradeInstalled {
            upgrade: UpgradeId::DigitalFeedwater,
            installed: true,
        },
    );
    // Uninstalling a module that is in AUTO drops it to MANUAL first.
    op(
        &mut p,
        Command::SetModuleInstalled {
            module: ModuleId::FeedwaterLevel,
            installed: false,
        },
    );
    assert_eq!(
        p.automation.mode(ModuleId::FeedwaterLevel),
        ModuleMode::Manual
    );
    let auto = Command::SetModuleMode {
        module: ModuleId::FeedwaterLevel,
        mode: ModuleMode::Auto,
    };
    assert!(p.command(&auto, &Origin::Operator).is_err());
    op(
        &mut p,
        Command::SetModuleInstalled {
            module: ModuleId::FeedwaterLevel,
            installed: true,
        },
    );
    op(&mut p, auto);
    // Removing the turbine computer drops the turbine-side automation.
    op(
        &mut p,
        Command::SetUpgradeInstalled {
            upgrade: UpgradeId::TurbineControlComputer,
            installed: false,
        },
    );
    assert_eq!(
        p.automation.mode(ModuleId::TurbineGovernor),
        ModuleMode::Manual
    );
    assert_eq!(p.automation.mode(ModuleId::Excitation), ModuleMode::Manual);
    // Original plant preset strips everything; it survives a scenario reload.
    op(
        &mut p,
        Command::SetControlRoom {
            generation: ControlRoomGeneration::Analog,
        },
    );
    assert!(p.automation.upgrades.is_empty() && p.automation.owned.is_empty());
    op(
        &mut p,
        Command::LoadScenario {
            name: "hot_standby".into(),
        },
    );
    assert!(p.automation.upgrades.is_empty());
    assert!(p
        .automation
        .modes
        .values()
        .all(|m| *m == ModuleMode::Manual));
    let t = p.telemetry(1.0, false);
    assert!(t
        .sections
        .values()
        .all(|g| *g == ControlRoomGeneration::Analog));
}
