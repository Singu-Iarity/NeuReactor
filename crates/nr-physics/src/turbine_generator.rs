//! Turbine-generator shaft, generator voltage and grid synchronisation.
//!
//! A 4-pole (1800 rpm, 60 Hz) machine. While the main breaker is open the
//! shaft accelerates on the balance of steam power and windage/friction
//! losses; once closed it is locked to grid frequency. The phase angle between
//! generator and grid voltage is integrated continuously, which is what the
//! synchroscope shows.

use nr_core::{clamp, Rng};
use serde::{Deserialize, Serialize};

pub const RATED_RPM: f64 = 1800.0;
pub const RATED_HZ: f64 = 60.0;
pub const RATED_KV: f64 = 22.0;

/// Sync window limits.
pub const SYNC_MAX_PHASE_DEG: f64 = 10.0;
pub const SYNC_MAX_SLIP_HZ: f64 = 0.1;
pub const SYNC_MAX_DV_FRAC: f64 = 0.05;

/// Lockout (86G) below this generator health.
pub const LOCKOUT_HEALTH: f64 = 50.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CloseOutcome {
    /// Breaker closed cleanly (severity ≤ 1).
    Closed,
    /// Sync-check relay prevented the close.
    Blocked { severity: f64 },
    /// Closed out of the window: mechanical/electrical stress, some damage.
    Rough { severity: f64, damage: f64 },
    /// Closed far out of phase: generator protection tripped the unit.
    Tripped { severity: f64, damage: f64 },
    /// Close refused by an interlock.
    Refused(&'static str),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurbineGenerator {
    /// Machine rating [W] (MVA treated as MW for inertia).
    pub rating: f64,
    /// Inertia constant H [s].
    pub inertia_h: f64,
    /// Shaft speed as a fraction of rated.
    pub speed: f64,
    /// Generator-to-grid phase angle [deg], (-180, 180].
    pub phase: f64,
    pub grid_hz: f64,
    pub grid_kv: f64,
    /// Exciter output 0..1 (0.5 gives rated voltage at rated speed).
    pub excitation: f64,
    pub field_breaker: bool,
    pub breaker: bool,
    /// Generator condition 0..100 %.
    pub health: f64,
    pub lockout: bool,
    /// Net shaft power (mechanical − losses) last step [W].
    pub net_power: f64,
    reverse_power_timer: f64,
}

impl TurbineGenerator {
    /// Synchronised and loaded, as at full power.
    pub fn synced(rating: f64) -> Self {
        Self {
            rating,
            inertia_h: 4.0,
            speed: 1.0,
            phase: 0.0,
            grid_hz: RATED_HZ,
            grid_kv: RATED_KV,
            excitation: 0.5,
            field_breaker: true,
            breaker: true,
            health: 100.0,
            lockout: false,
            net_power: 0.0,
            reverse_power_timer: 0.0,
        }
    }

    pub fn rpm(&self) -> f64 {
        self.speed * RATED_RPM
    }

    pub fn gen_hz(&self) -> f64 {
        if self.breaker {
            self.grid_hz
        } else {
            self.speed * RATED_HZ
        }
    }

    pub fn slip_hz(&self) -> f64 {
        self.gen_hz() - self.grid_hz
    }

    /// Generator terminal voltage [kV].
    pub fn gen_kv(&self) -> f64 {
        if self.breaker {
            return self.grid_kv;
        }
        if !self.field_breaker {
            return 0.0;
        }
        RATED_KV * self.speed * self.excitation / 0.5
    }

    /// Windage and friction loss at the current speed [W].
    pub fn losses(&self) -> f64 {
        self.rating * (0.005 * self.speed.powi(3) + 0.001 * self.speed)
    }

    /// Worst normalised sync error; ≤ 1 means inside the sync window.
    pub fn sync_severity(&self) -> f64 {
        let dv = (self.gen_kv() - self.grid_kv).abs() / self.grid_kv;
        (self.phase.abs() / SYNC_MAX_PHASE_DEG)
            .max(self.slip_hz().abs() / SYNC_MAX_SLIP_HZ)
            .max(dv / SYNC_MAX_DV_FRAC)
    }

    pub fn in_sync_window(&self) -> bool {
        self.sync_severity() <= 1.0
    }

    /// Advance with mechanical turbine power `p_mech` [W]. Returns any
    /// protective action that occurred this step.
    pub fn step(&mut self, p_mech: f64, dt: f64, rng: &mut Rng) -> Option<&'static str> {
        // Grid frequency: slow Ornstein-Uhlenbeck wander around 60 Hz.
        let tau: f64 = 300.0;
        let sigma = 0.02 * (2.0 / tau).sqrt();
        self.grid_hz += -(self.grid_hz - RATED_HZ) / tau * dt + sigma * dt.sqrt() * rng.normal();
        self.grid_kv += -(self.grid_kv - RATED_KV) / tau * dt
            + 0.05 * (2.0 / tau).sqrt() * dt.sqrt() * rng.normal();

        self.net_power = p_mech - self.losses();
        if self.breaker {
            self.speed = self.grid_hz / RATED_HZ;
            self.phase = 0.0;
            if !self.field_breaker {
                self.breaker = false;
                return Some("Generator loss of field");
            }
            if self.net_power < 0.0 {
                self.reverse_power_timer += dt;
                if self.reverse_power_timer > 15.0 {
                    self.breaker = false;
                    self.reverse_power_timer = 0.0;
                    return Some("Generator reverse power");
                }
            } else {
                self.reverse_power_timer = 0.0;
            }
            return None;
        }
        self.reverse_power_timer = 0.0;
        // Kinetic energy E = H·S·n²  =>  dn/dt = P / (2·H·S·n)
        let ke = self.inertia_h * self.rating;
        let n = self.speed.max(0.02);
        self.speed = (self.speed + self.net_power / (2.0 * ke * n) * dt).max(0.0);
        self.phase += 360.0 * (self.gen_hz() - self.grid_hz) * dt;
        self.phase = (self.phase + 180.0).rem_euclid(360.0) - 180.0;
        if self.speed > 1.1 {
            return Some("Turbine overspeed");
        }
        None
    }

    /// Operator / auto-synchroniser closes the main breaker.
    pub fn close(&mut self, sync_check: bool) -> CloseOutcome {
        if self.breaker {
            return CloseOutcome::Closed;
        }
        if self.lockout {
            return CloseOutcome::Refused("generator lockout relay (86G) tripped");
        }
        if !self.field_breaker {
            return CloseOutcome::Refused("field breaker open");
        }
        let s = self.sync_severity();
        if s <= 1.0 {
            self.breaker = true;
            return CloseOutcome::Closed;
        }
        if sync_check {
            return CloseOutcome::Blocked { severity: s };
        }
        if s < 3.0 {
            let damage = (s - 1.0) * 2.0;
            self.damage(damage);
            self.breaker = !self.lockout;
            CloseOutcome::Rough {
                severity: s,
                damage,
            }
        } else {
            let damage = clamp(s * 5.0, 0.0, 60.0);
            self.damage(damage);
            CloseOutcome::Tripped {
                severity: s,
                damage,
            }
        }
    }

    pub fn open(&mut self) {
        self.breaker = false;
    }

    fn damage(&mut self, amount: f64) {
        self.health = (self.health - amount).max(0.0);
        if self.health < LOCKOUT_HEALTH {
            self.lockout = true;
        }
    }

    /// Restore the generator (sandbox; career will charge for this).
    pub fn repair(&mut self) {
        self.health = 100.0;
        self.lockout = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rolling() -> TurbineGenerator {
        let mut tg = TurbineGenerator::synced(1300e6);
        tg.breaker = false;
        tg
    }

    #[test]
    fn steam_accelerates_and_losses_decelerate() {
        let mut rng = Rng::new(1);
        let mut tg = rolling();
        tg.speed = 0.5;
        for _ in 0..200 {
            tg.step(30e6, 0.05, &mut rng);
        }
        assert!(tg.speed > 0.5);
        let s = tg.speed;
        for _ in 0..200 {
            tg.step(0.0, 0.05, &mut rng);
        }
        assert!(tg.speed < s);
    }

    #[test]
    fn synchroscope_rotates_fast_when_generator_fast() {
        let mut rng = Rng::new(2);
        let mut tg = rolling();
        tg.speed = 1.0 + 0.05 / 60.0; // +0.05 Hz
        let hold = tg.losses(); // keep speed constant
        let mut prev = tg.phase;
        let mut advanced = 0.0;
        for _ in 0..100 {
            tg.step(hold, 0.05, &mut rng);
            let mut d = tg.phase - prev;
            if d < -180.0 {
                d += 360.0;
            }
            advanced += d;
            prev = tg.phase;
        }
        // 5 s at ~0.05 Hz slip ≈ 90°, grid wander adds a little noise.
        assert!((60.0..120.0).contains(&advanced), "advanced {advanced}");
    }

    #[test]
    fn close_in_window_and_out_of_window() {
        let mut tg = rolling();
        tg.phase = 3.0;
        assert_eq!(tg.close(true), CloseOutcome::Closed);

        let mut tg = rolling();
        tg.phase = 90.0;
        assert!(matches!(tg.close(true), CloseOutcome::Blocked { .. }));
        assert!(!tg.breaker);
        match tg.close(false) {
            CloseOutcome::Tripped { damage, .. } => assert!(damage > 20.0),
            o => panic!("{o:?}"),
        }
        assert!(!tg.breaker && tg.health < 80.0);

        let mut tg = rolling();
        tg.phase = 15.0;
        assert!(matches!(tg.close(false), CloseOutcome::Rough { .. }));
        assert!(tg.breaker);
    }

    #[test]
    fn heavy_damage_locks_out_until_repair() {
        let mut tg = rolling();
        tg.phase = 180.0;
        tg.close(false);
        tg.phase = 180.0;
        tg.close(false);
        assert!(tg.lockout);
        tg.phase = 0.0;
        assert!(matches!(tg.close(false), CloseOutcome::Refused(_)));
        tg.repair();
        assert_eq!(tg.close(false), CloseOutcome::Closed);
    }

    #[test]
    fn overspeed_trips() {
        let mut rng = Rng::new(3);
        let mut tg = rolling();
        let mut tripped = None;
        for _ in 0..20_000 {
            if let Some(t) = tg.step(300e6, 0.05, &mut rng) {
                tripped = Some(t);
                break;
            }
        }
        assert_eq!(tripped, Some("Turbine overspeed"));
    }

    #[test]
    fn reverse_power_opens_breaker() {
        let mut rng = Rng::new(4);
        let mut tg = TurbineGenerator::synced(1300e6);
        let mut out = None;
        for _ in 0..400 {
            if let Some(t) = tg.step(0.0, 0.05, &mut rng) {
                out = Some(t);
                break;
            }
        }
        assert_eq!(out, Some("Generator reverse power"));
        assert!(!tg.breaker);
    }
}
