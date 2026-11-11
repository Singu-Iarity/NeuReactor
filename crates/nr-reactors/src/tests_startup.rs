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
    // Default acceleration is 120 rpm/min: about 15 minutes to 1800 rpm.
    p.advance(1100.0);
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
    // Crack the valve open a little so the machine runs fast and the
    // synchroscope sweeps round.
    let v = p.turbine_valve + 0.004;
    op(&mut p, Command::SetTurbineValve { fraction: v });
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

#[test]
fn component_quality_changes_output() {
    use nr_api::equipment::{Era, Slot, Tier};
    let run = |era, tier| {
        let mut p = Pwr::full_power();
        // Hold the turbine valve so output reflects efficiency, not the governor.
        op(
            &mut p,
            Command::SetModuleMode {
                module: ModuleId::TurbineGovernor,
                mode: ModuleMode::Manual,
            },
        );
        for slot in [Slot::Turbine, Slot::Generator, Slot::Condenser] {
            op(&mut p, Command::SetComponent { slot, era, tier });
        }
        p.advance(300.0);
        p.gross_mwe
    };
    let vintage = run(Era::Vintage, Tier::Basic);
    let standard = run(Era::Dated, Tier::Quality);
    let modern = run(Era::Modern, Tier::Premium);
    assert!(
        vintage < standard - 40.0 && modern > standard + 30.0,
        "{vintage} {standard} {modern}"
    );
}

#[test]
fn analog_controllers_are_less_precise() {
    use nr_api::equipment::{Controller, Era, Tier};
    let spread = |c: Controller| {
        let mut p = Pwr::full_power();
        op(
            &mut p,
            Command::SetModuleController {
                module: ModuleId::PressurizerPressure,
                controller: c,
            },
        );
        op(
            &mut p,
            Command::SetModuleMode {
                module: ModuleId::PressurizerPressure,
                mode: ModuleMode::Auto,
            },
        );
        let mut worst: f64 = 0.0;
        for _ in 0..120 {
            p.advance(30.0);
            worst = worst.max((p.pressure / 1e6 - 15.51).abs());
        }
        worst
    };
    let digital = spread(Controller::Digital);
    let analog = spread(Controller::Analog {
        era: Era::Vintage,
        tier: Tier::Basic,
    });
    assert!(analog > 2.0 * digital, "analog {analog} digital {digital}");
}

#[test]
fn analog_controller_runs_without_computer() {
    use nr_api::equipment::{Controller, Era, Tier};
    let mut p = Pwr::full_power();
    op(
        &mut p,
        Command::SetControlRoom {
            generation: ControlRoomGeneration::Analog,
        },
    );
    op(
        &mut p,
        Command::SetModuleInstalled {
            module: ModuleId::RodControl,
            installed: true,
        },
    );
    let auto = Command::SetModuleMode {
        module: ModuleId::RodControl,
        mode: ModuleMode::Auto,
    };
    assert!(
        p.command(&auto, &Origin::Operator).is_err(),
        "digital needs the computer"
    );
    op(
        &mut p,
        Command::SetModuleController {
            module: ModuleId::RodControl,
            controller: Controller::Analog {
                era: Era::Dated,
                tier: Tier::Quality,
            },
        },
    );
    op(&mut p, auto);
}

#[test]
fn era_mismatch_fits_conversion_kit_in_sandbox() {
    use nr_api::equipment::{Era, Slot, Tier};
    let mut p = Pwr::full_power();
    op(
        &mut p,
        Command::SetComponent {
            slot: Slot::Generator,
            era: Era::Vintage,
            tier: Tier::Basic,
        },
    );
    op(
        &mut p,
        Command::SetComponent {
            slot: Slot::Turbine,
            era: Era::Modern,
            tier: Tier::Quality,
        },
    );
    let t = p.telemetry(1.0, false);
    assert!(t
        .required_kits
        .iter()
        .any(|k| k.key == "couple:Turbine:Generator"));
    assert!(p.automation.equipment.missing_kits().is_empty());
}

/// The reported bug: turbine rolled to 1800 rpm with the reactor
/// subcritical. Decay heat and stored SG energy must not sustain it.
#[test]
fn turbine_cannot_roll_on_decay_heat() {
    let mut p = Pwr::hot_standby();
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "accel_rpm_min".into(),
            value: 180.0,
        },
    );
    // Vacuum first (auto module), then latch and ask for 1800 rpm.
    p.advance(3600.0);
    assert!(
        p.condenser.pressure_kpa < 25.0,
        "vacuum {}",
        p.condenser.pressure_kpa
    );
    op(&mut p, Command::ResetTurbine);
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::TurbineGovernor,
            key: "speed_rpm".into(),
            value: 1800.0,
        },
    );
    p.advance(1800.0);
    let rpm = p.tg.rpm();
    assert!(p.kinetics.n < 1e-6);
    assert!(
        rpm < 1750.0 || p.steam_pressure() < 5.0e6,
        "rpm {rpm}, steam {} MPa",
        p.steam_pressure() / 1e6
    );
}

#[test]
fn shutdown_banks_must_be_withdrawn_first() {
    let mut p = Pwr::hot_standby();
    let out = Command::RodMotion {
        selection: "manual".into(),
        direction: RodDirection::Out,
    };
    assert!(p.command(&out, &Origin::Operator).is_err());
    assert!(p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "rod_block"));
    let mv = Command::MoveRodBank {
        bank: "D".into(),
        target: 50.0,
    };
    assert!(p.command(&mv, &Origin::Operator).is_err());
    for bank in ["SA", "SB"] {
        lever(&mut p, bank, RodDirection::Out);
        p.advance(300.0);
    }
    op(&mut p, out);
}

#[test]
fn no_vacuum_means_no_turbine_and_no_dumps() {
    let mut p = Pwr::hot_standby();
    for m in [ModuleId::CondenserVacuum, ModuleId::SteamDump] {
        op(
            &mut p,
            Command::SetModuleMode {
                module: m,
                mode: ModuleMode::Manual,
            },
        );
    }
    assert!(p
        .command(&Command::ResetTurbine, &Origin::Operator)
        .is_err());
    let t = p.telemetry(1.0, false);
    assert!(t.alarms.iter().any(|a| a.tag == "vac_required"));
    op(&mut p, Command::SetSteamDump { fraction: 0.5 });
    p.advance(60.0);
    assert!(p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "dump_blocked"));
    assert!(
        p.steam_flow < 50.0,
        "only ARVs may pass steam: {}",
        p.steam_flow
    );
    // Manual pulldown with hogging pumps then SJAEs.
    for i in 0..2 {
        op(
            &mut p,
            Command::SetVacuumPump {
                index: i,
                running: true,
            },
        );
    }
    op(&mut p, Command::SetSjae { on: true });
    p.advance(2400.0);
    assert!(
        p.condenser.pressure_kpa < 25.0,
        "{}",
        p.condenser.pressure_kpa
    );
    op(&mut p, Command::ResetTurbine);
}

#[test]
fn closing_msivs_stops_steam_and_trips_on_low_vacuum_eventually() {
    let mut p = Pwr::full_power();
    op(&mut p, Command::SetMsiv { open: false });
    p.advance(5.0);
    assert!(p.reactor_tripped || p.turbine_tripped || p.steam_flow < 1.0);
    assert!(p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "msiv_closed"));
}

#[test]
fn deaeration_needs_pegging_steam_at_low_load() {
    let mut p = Pwr::hot_standby();
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::Deaerator,
            mode: ModuleMode::Manual,
        },
    );
    p.advance(600.0);
    assert!(p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "deaeration"));
    op(&mut p, Command::SetPeggingSteam { open: true });
    p.advance(900.0);
    assert!(!p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "deaeration"));
}

#[test]
fn rod_group_deviation_tilts_power_and_balancer_fixes_it() {
    let mut p = Pwr::full_power();
    // Turn the balancer off and knock group D2 out of step.
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::RodControl,
            key: "balancer_speed".into(),
            value: 0.0,
        },
    );
    p.banks[5].move_group(2, -40.0);
    p.advance(20.0);
    let t = p.telemetry(1.0, false);
    assert!(t.alarms.iter().any(|a| a.tag == "rod_dev"));
    assert!(
        t.alarms.iter().any(|a| a.tag == "qptr"),
        "tilt {:?}",
        t.quadrant_tilt
    );
    let n41 = t.readings["pr_n41_pct"];
    let n42 = t.readings["pr_n42_pct"];
    assert!(n41 > n42 + 1.0, "N41 {n41} N42 {n42}");
    // Balancer at FAST realigns the groups.
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::RodControl,
            key: "balancer_speed".into(),
            value: 3.0,
        },
    );
    p.advance(200.0);
    assert!(
        p.banks[5].group2_offset.abs() < 0.5,
        "{}",
        p.banks[5].group2_offset
    );
    assert!(!p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "rod_dev"));
}

#[test]
fn group_select_needs_the_pushbutton_grid_and_speed_selector_works() {
    let mut p = Pwr::hot_standby();
    op(
        &mut p,
        Command::SetRodSpeed {
            steps_per_min: 72.0,
        },
    );
    lever(&mut p, "SA", RodDirection::Out);
    p.advance(60.0);
    assert!(
        (p.banks[0].position - 72.0).abs() < 2.0,
        "SA {}",
        p.banks[0].position
    );
    lever(&mut p, "SA", RodDirection::Hold);
    lever(&mut p, "SA2", RodDirection::Out);
    p.advance(10.0);
    lever(&mut p, "SA2", RodDirection::Hold);
    assert!(p.banks[0].group2() > p.banks[0].position + 5.0);
    op(
        &mut p,
        Command::SetUpgradeInstalled {
            upgrade: UpgradeId::RodGroupSelect,
            installed: false,
        },
    );
    let r = p.command(
        &Command::RodMotion {
            selection: "SA2".into(),
            direction: RodDirection::Out,
        },
        &Origin::Operator,
    );
    assert!(r.is_err());
}

fn pull_shutdown_banks(p: &mut Pwr) {
    op(
        p,
        Command::SetRodSpeed {
            steps_per_min: 72.0,
        },
    );
    for bank in ["SA", "SB"] {
        lever(p, bank, RodDirection::Out);
        p.advance(200.0);
        lever(p, bank, RodDirection::Hold);
    }
}

/// Advance while doing what an operator must: block the SR at P-6.
fn run_blocking_sr(p: &mut Pwr, secs: f64) {
    let mut t = 0.0;
    while t < secs {
        p.advance(10.0);
        t += 10.0;
        let tel = p.telemetry(1.0, false);
        if tel.nis.p6 && !tel.nis.sr_blocked {
            op(
                p,
                Command::BlockNis {
                    channel: NisBlock::SourceRange,
                    blocked: true,
                },
            );
        }
        assert!(!p.reactor_tripped, "trip: {:?}", p.trips);
    }
}

#[test]
fn reactor_power_controller_takes_reactor_to_setpoint() {
    let mut p = Pwr::hot_standby();
    pull_shutdown_banks(&mut p);
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::RodControl,
            mode: ModuleMode::Auto,
        },
    );
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::ReactorPower,
            mode: ModuleMode::Auto,
        },
    );
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::ReactorPower,
            key: "power_pct".into(),
            value: 5.0,
        },
    );
    run_blocking_sr(&mut p, 5400.0);
    let pw = p.kinetics.n * 100.0;
    assert!((3.5..6.5).contains(&pw), "power {pw}");
    assert!(p
        .telemetry(1.0, false)
        .alarms
        .iter()
        .any(|a| a.tag == "rpc_auto"));
}

#[test]
fn coordinated_mode_borates_to_bring_rods_up() {
    let mut p = Pwr::hot_standby();
    pull_shutdown_banks(&mut p);
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::RodControl,
            mode: ModuleMode::Auto,
        },
    );
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::ReactorPower,
            mode: ModuleMode::Auto,
        },
    );
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::ReactorPower,
            key: "mode".into(),
            value: 2.0,
        },
    );
    op(
        &mut p,
        Command::SetSetpoint {
            module: ModuleId::ReactorPower,
            key: "power_pct".into(),
            value: 3.0,
        },
    );
    let b0 = p.boron_ppm;
    run_blocking_sr(&mut p, 3.0 * 3600.0);
    // At low power the control banks sit deep: boron is added so rod control
    // withdraws them back into the D-bank band while power holds.
    assert!(p.boron_ppm > b0 + 20.0, "boron {b0} -> {}", p.boron_ppm);
    let d = p.banks[5].position;
    assert!((140.0..=228.0).contains(&d), "D bank {d}");
    assert!(
        (2.0..4.5).contains(&(p.kinetics.n * 100.0)),
        "power {}",
        p.kinetics.n * 100.0
    );
}

#[test]
fn agc_needs_unit_load_controller_and_dispatch_link() {
    let mut p = Pwr::full_power();
    assert!(!p.agc_available());
    op(
        &mut p,
        Command::SetModuleMode {
            module: ModuleId::PlantMaster,
            mode: ModuleMode::Auto,
        },
    );
    assert!(p.agc_available());
    op(
        &mut p,
        Command::SetUpgradeInstalled {
            upgrade: UpgradeId::GridDispatchLink,
            installed: false,
        },
    );
    assert!(!p.agc_available());
}
