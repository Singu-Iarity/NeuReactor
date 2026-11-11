use super::*;

fn equip_all(c: &mut Career, era: Era, tier: Tier, plant: &mut Pwr) {
    for s in equipment::SLOTS {
        c.buy(
            Purchase::Component {
                slot: s.slot,
                era,
                tier,
            },
            plant,
        )
        .unwrap();
    }
}

#[test]
fn start_year_gates_technology() {
    let c = Career::new(1972, Difficulty::Normal);
    let mut plant = Pwr::hot_standby();
    let mut c2 = c.clone();
    assert!(c2
        .buy(
            Purchase::Component {
                slot: Slot::Turbine,
                era: Era::Dated,
                tier: Tier::Quality
            },
            &mut plant
        )
        .is_err());
    assert!(c2
        .buy(
            Purchase::Component {
                slot: Slot::Turbine,
                era: Era::Vintage,
                tier: Tier::Premium
            },
            &mut plant
        )
        .is_ok());
    assert!(c2
        .buy(
            Purchase::Upgrade {
                upgrade: UpgradeId::PlantProcessComputer
            },
            &mut plant
        )
        .is_err());
    assert!(c2
        .buy(
            Purchase::Controller {
                module: ModuleId::RodControl,
                controller: Controller::Analog {
                    era: Era::Vintage,
                    tier: Tier::Premium
                }
            },
            &mut plant
        )
        .is_ok());
}

#[test]
fn budget_forces_trade_offs() {
    let mut plant = Pwr::hot_standby();
    let mut c = Career::new(2000, Difficulty::Hard);
    // Modern premium everything is far beyond a hard budget.
    let total: f64 = equipment::SLOTS
        .iter()
        .map(|s| {
            c.quote(Purchase::Component {
                slot: s.slot,
                era: Era::Modern,
                tier: Tier::Premium,
            })
            .total
        })
        .sum();
    assert!(total > c.money);
    equip_all(&mut c, Era::Vintage, Tier::Basic, &mut plant);
    assert!(c.money > 0.0);
}

#[test]
fn build_phase_swaps_are_refunded_and_kits_are_charged() {
    let mut plant = Pwr::hot_standby();
    let mut c = Career::new(2000, Difficulty::Easy);
    let m0 = c.money;
    c.buy(
        Purchase::Component {
            slot: Slot::Turbine,
            era: Era::Dated,
            tier: Tier::Quality,
        },
        &mut plant,
    )
    .unwrap();
    c.buy(
        Purchase::Component {
            slot: Slot::Turbine,
            era: Era::Vintage,
            tier: Tier::Basic,
        },
        &mut plant,
    )
    .unwrap();
    let vb = Component {
        era: Era::Vintage,
        tier: Tier::Basic,
    }
    .price(Slot::Turbine);
    assert!((m0 - c.money - vb).abs() < 1.0);
    // A modern generator on a vintage turbine needs an adapter.
    let q = c.quote(Purchase::Component {
        slot: Slot::Generator,
        era: Era::Modern,
        tier: Tier::Quality,
    });
    assert_eq!(q.kits.len(), 1);
    assert!(q.total > q.price);
}

#[test]
fn commission_operate_and_earn() {
    let mut plant = Pwr::hot_standby();
    let mut c = Career::new(1990, Difficulty::Normal);
    assert!(c.commission().is_err(), "needs every component");
    equip_all(&mut c, Era::Dated, Tier::Quality, &mut plant);
    let mut plant = c.commission().unwrap();
    assert_eq!(plant.automation.game_mode, GameMode::Career);
    assert!(plant.automation.owned.is_empty());
    // Pretend the unit runs at full power for a day.
    let fp = Pwr::full_power();
    let before = c.money;
    for _ in 0..24 {
        c.tick(3600.0, &fp);
    }
    let expected = (fp.gross_mwe - fp.p.house_load_mwe) * 24.0 * c.price_mwh - c.om_per_day;
    assert!((c.money - before - expected).abs() / expected < 0.01);
    // Components can't be swapped while generating...
    let mut running = Pwr::full_power();
    assert!(c
        .buy(
            Purchase::Component {
                slot: Slot::Turbine,
                era: Era::Dated,
                tier: Tier::Premium
            },
            &mut running
        )
        .is_err());
    // ...but controllers can, and they reach the plant.
    c.buy(
        Purchase::Controller {
            module: ModuleId::RodControl,
            controller: Controller::Analog {
                era: Era::Dated,
                tier: Tier::Quality,
            },
        },
        &mut plant,
    )
    .unwrap();
    assert!(plant.automation.owned.contains(&ModuleId::RodControl));
}

#[test]
fn calendar_announces_new_technology_and_research_unlocks_early() {
    let mut plant = Pwr::hot_standby();
    let mut c = Career::new(1978, Difficulty::Easy);
    equip_all(&mut c, Era::Vintage, Tier::Quality, &mut plant);
    let _ = c.commission().unwrap();
    let idle = Pwr::hot_standby();
    // Research the plant process computer early (prereq: digital software,
    // which needs dated analog controllers: available 1978).
    assert!(c
        .start_research(Unlock::Upgrade {
            upgrade: UpgradeId::PlantProcessComputer
        })
        .is_err());
    c.start_research(Unlock::DigitalControllers).unwrap();
    let days = c.research[0].days;
    c.tick(days * DAY + 1.0, &idle);
    assert!(c.available(Unlock::DigitalControllers));
    assert!(c
        .notifications
        .iter()
        .any(|n| n.text.starts_with("Research complete")));
    // Two years later dated turbines appear by themselves.
    c.tick(2.5 * 365.0 * DAY, &idle);
    assert!(c.available(Unlock::Component {
        slot: Slot::Turbine,
        era: Era::Dated
    }));
    assert!(c.notifications.iter().any(|n| n
        .text
        .contains("Due to new research, you can now install Dated Main Turbine")));
    assert!(c.date().ends_with("1980") || c.date().ends_with("1981"));
}
