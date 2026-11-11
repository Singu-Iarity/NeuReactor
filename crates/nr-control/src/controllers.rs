//! Automation module controllers.

use nr_core::{clamp, lerp, Pi};

/// Automatic rod control: Tavg/Tref error plus power-mismatch, with a
/// deadband and a variable-speed program (8–72 steps/min).
#[derive(Debug, Clone)]
pub struct RodControl {
    pub deadband: f64,
    pub lockup: f64,
    /// Power mismatch gain [K per unit power fraction].
    pub mismatch_gain: f64,
    moving: bool,
    last_turbine: f64,
    mismatch: f64,
}

impl Default for RodControl {
    fn default() -> Self {
        Self {
            deadband: 0.8,
            lockup: 0.5,
            mismatch_gain: 5.0,
            moving: false,
            last_turbine: 0.0,
            mismatch: 0.0,
        }
    }
}

impl RodControl {
    /// Returns rod speed demand in steps/s, positive = withdraw.
    pub fn update(&mut self, tavg: f64, tref: f64, nuclear: f64, turbine: f64, dt: f64) -> f64 {
        // Rate-sensitive mismatch channel: reacts to turbine load changes,
        // decays away over ~80 s (lead/lag in real plants).
        let d_turbine = turbine - self.last_turbine;
        self.last_turbine = turbine;
        self.mismatch += d_turbine;
        self.mismatch *= (-dt / 80.0f64).exp();
        let _ = nuclear;
        let error = (tavg - tref) - self.mismatch_gain * self.mismatch;
        let threshold = if self.moving {
            self.lockup
        } else {
            self.deadband
        };
        if error.abs() < threshold {
            self.moving = false;
            return 0.0;
        }
        self.moving = true;
        // 8 steps/min at deadband -> 72 steps/min at >= 2.8 K error.
        let spm = lerp(
            8.0,
            72.0,
            clamp((error.abs() - self.deadband) / 2.0, 0.0, 1.0),
        );
        -error.signum() * spm / 60.0
    }

    pub fn reset(&mut self, turbine: f64) {
        self.last_turbine = turbine;
        self.mismatch = 0.0;
        self.moving = false;
    }
}

/// Moves a set of control banks with overlap given a single speed demand.
/// `positions` ordered A, B, C, D. Returns per-bank target deltas.
pub fn overlap_sequence(
    positions: &[f64],
    max_steps: f64,
    overlap: f64,
    speed: f64,
    dt: f64,
) -> Vec<f64> {
    let n = positions.len();
    let mut delta = vec![0.0; n];
    let step = speed * dt;
    if step > 0.0 {
        for k in 0..n {
            let prev_ok = k == 0 || positions[k - 1] >= max_steps - overlap;
            if prev_ok && positions[k] < max_steps {
                delta[k] = step;
            }
        }
    } else if step < 0.0 {
        for k in 0..n {
            let next_ok = k == n - 1 || positions[k + 1] < max_steps - overlap;
            if next_ok && positions[k] > 0.0 {
                delta[k] = step;
            }
        }
    }
    delta
}

/// Pressurizer pressure: proportional/backup heaters and spray.
#[derive(Debug, Clone)]
pub struct PressureControl {
    pi: Pi,
}

impl Default for PressureControl {
    fn default() -> Self {
        // Output: heater fraction 0..1 for negative error (pressure low),
        // spray fraction for positive "master" output below zero.
        Self {
            pi: Pi::new(4.0, 0.02, -1.0, 1.0),
        }
    }
}

impl PressureControl {
    /// `p` and `setpoint` in MPa. Returns (heater fraction, spray fraction).
    pub fn update(&mut self, p: f64, setpoint: f64, dt: f64) -> (f64, f64) {
        let master = self.pi.update(setpoint - p, dt);
        let heaters = clamp(master * 2.0 + 0.1, 0.0, 1.0);
        let spray = clamp(-master * 2.0 - 0.1, 0.0, 1.0);
        (heaters, spray)
    }

    pub fn track(&mut self, heaters: f64) {
        self.pi.track((heaters - 0.1) / 2.0);
    }
}

/// Pressurizer level via charging flow.
#[derive(Debug, Clone)]
pub struct LevelControl {
    pi: Pi,
}

impl LevelControl {
    pub fn new(max_flow: f64) -> Self {
        Self {
            pi: Pi::new(1.0, 0.01, 0.0, max_flow),
        }
    }

    pub fn update(&mut self, level: f64, setpoint: f64, letdown: f64, dt: f64) -> f64 {
        self.pi.bias = letdown;
        self.pi.update(setpoint - level, dt)
    }

    pub fn track(&mut self, charging: f64, letdown: f64) {
        self.pi.bias = letdown;
        self.pi.track(charging);
    }
}

/// Three-element feedwater control: feed demand = steam flow + level PI.
#[derive(Debug, Clone)]
pub struct FeedwaterControl {
    pi: Pi,
}

impl Default for FeedwaterControl {
    fn default() -> Self {
        Self {
            pi: Pi::new(0.01, 0.0002, -0.5, 0.5),
        }
    }
}

impl FeedwaterControl {
    /// Returns feed demand as a fraction of max feed flow.
    pub fn update(&mut self, level: f64, setpoint: f64, steam_frac: f64, dt: f64) -> f64 {
        clamp(steam_frac + self.pi.update(setpoint - level, dt), 0.0, 1.0)
    }

    pub fn track(&mut self, feed: f64, steam_frac: f64) {
        self.pi.track(feed - steam_frac);
    }
}

/// Steam dump: load-rejection mode while at power, plant-trip mode (hold
/// no-load Tavg) after a reactor trip.
#[derive(Debug, Clone)]
pub struct SteamDumpControl {
    pi: Pi,
}

impl Default for SteamDumpControl {
    fn default() -> Self {
        Self {
            pi: Pi::new(0.15, 0.002, 0.0, 1.0),
        }
    }
}

impl SteamDumpControl {
    pub fn update(&mut self, tavg: f64, tref: f64, noload: f64, tripped: bool, dt: f64) -> f64 {
        let target = if tripped { noload } else { tref + 2.8 };
        self.pi.update(tavg - target, dt)
    }
}

/// Turbine governor. With the generator breaker open it is a speed
/// controller (ramping a speed reference at the selected acceleration);
/// with the breaker closed it ramps a load reference and trims the governor
/// valves on electrical output.
#[derive(Debug, Clone)]
pub struct TurbineGovernor {
    pub load_ref: f64,
    pub speed_ref: f64,
    load_pi: Pi,
    speed_pi: Pi,
}

impl Default for TurbineGovernor {
    fn default() -> Self {
        Self {
            load_ref: 0.0,
            speed_ref: 0.0,
            load_pi: Pi::new(0.0005, 0.0005, 0.0, 1.0),
            speed_pi: Pi::new(0.0005, 0.00005, 0.0, 0.15),
        }
    }
}

impl TurbineGovernor {
    /// Load control. Returns governor valve demand 0..1.
    pub fn update(&mut self, target_mwe: f64, ramp_mw_min: f64, mwe: f64, dt: f64) -> f64 {
        let max = ramp_mw_min / 60.0 * dt;
        self.load_ref += clamp(target_mwe - self.load_ref, -max, max);
        self.load_pi.update(self.load_ref - mwe, dt)
    }

    /// Speed control. Returns governor valve demand 0..1.
    pub fn update_speed(&mut self, target_rpm: f64, accel_rpm_min: f64, rpm: f64, dt: f64) -> f64 {
        let max = accel_rpm_min / 60.0 * dt;
        self.speed_ref += clamp(target_rpm - self.speed_ref, -max, max);
        self.speed_pi.update(self.speed_ref - rpm, dt)
    }

    pub fn track(&mut self, valve: f64, mwe: f64) {
        self.load_ref = mwe;
        self.load_pi.track(valve);
    }

    pub fn track_speed(&mut self, valve: f64, rpm: f64) {
        self.speed_ref = rpm;
        self.speed_pi.track(valve.min(0.15));
    }
}

/// Automatic voltage regulator: trims excitation to hold terminal voltage.
#[derive(Debug, Clone)]
pub struct Avr {
    pi: Pi,
}

impl Default for Avr {
    fn default() -> Self {
        Self {
            pi: Pi::new(0.005, 0.02, 0.0, 1.0),
        }
    }
}

impl Avr {
    pub fn update(&mut self, target_kv: f64, kv: f64, dt: f64) -> f64 {
        self.pi.update(target_kv - kv, dt)
    }

    pub fn track(&mut self, excitation: f64) {
        self.pi.track(excitation);
    }
}

/// What the auto-synchronizer wants this step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncAction {
    /// Governor speed setpoint [rpm] (generator slightly fast).
    pub speed_rpm: f64,
    /// Voltage target for the AVR [kV].
    pub voltage_kv: f64,
    /// Issue the breaker close now.
    pub close: bool,
}

/// Auto-synchronizer: runs the generator slightly fast of grid frequency,
/// matches voltage and closes the breaker just before the phase crosses zero.
#[derive(Debug, Clone, Default)]
pub struct AutoSynchronizer;

impl AutoSynchronizer {
    pub const SLIP_TARGET_HZ: f64 = 0.04;

    pub fn update(
        &self,
        grid_hz: f64,
        grid_kv: f64,
        gen_kv: f64,
        phase_deg: f64,
        slip_hz: f64,
    ) -> SyncAction {
        let speed_rpm = (grid_hz + Self::SLIP_TARGET_HZ) / 60.0 * 1800.0;
        let dv_ok = (gen_kv - grid_kv).abs() / grid_kv < 0.03;
        let slip_ok = slip_hz > 0.005 && slip_hz < 0.08;
        // Generator fast => phase increases; close as it approaches 0 from below
        // (breaker closing time lead).
        let close = dv_ok && slip_ok && (-6.0..=1.0).contains(&phase_deg);
        SyncAction {
            speed_rpm,
            voltage_kv: grid_kv,
            close,
        }
    }
}

/// Boron/makeup automation keeps the lead control bank inside a band.
#[derive(Debug, Clone, Default)]
pub struct BoronControl {
    state: i8,
}

impl BoronControl {
    /// Returns -1 dilute, 0 off, +1 borate.
    ///
    /// Rods too deep mean the core has excess reactivity: borate, so rod
    /// control withdraws them. Rods too high: dilute, so they drive in.
    pub fn update(&mut self, lead_bank: f64, low: f64, high: f64) -> i8 {
        let mid = 0.5 * (low + high);
        self.state = match self.state {
            0 if lead_bank < low => 1,
            0 if lead_bank > high => -1,
            1 if lead_bank >= mid => 0,
            -1 if lead_bank <= mid => 0,
            s => s,
        };
        self.state
    }
}

/// Plant master: follows a dispatch target and runs back load on large
/// deviations of plant parameters.
#[derive(Debug, Clone, Default)]
pub struct PlantMaster;

impl PlantMaster {
    /// Returns the turbine load target [MWe].
    pub fn update(
        &self,
        target_mwe: f64,
        current_ref: f64,
        tavg_error: f64,
        pressure_error: f64,
    ) -> f64 {
        if tavg_error.abs() > 3.0 || pressure_error.abs() > 0.4 {
            // Hold load until the plant recovers.
            current_ref
        } else {
            target_mwe
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rods_insert_when_hot() {
        let mut rc = RodControl::default();
        rc.reset(1.0);
        assert!(rc.update(310.0, 308.0, 1.0, 1.0, 0.1) < 0.0);
        rc.reset(1.0);
        assert!(rc.update(306.0, 308.0, 1.0, 1.0, 0.1) > 0.0);
        rc.reset(1.0);
        assert_eq!(rc.update(308.3, 308.0, 1.0, 1.0, 0.1), 0.0);
    }

    #[test]
    fn overlap_moves_correct_banks() {
        // D leads on insertion.
        let d = overlap_sequence(&[228.0, 228.0, 228.0, 200.0], 228.0, 100.0, -1.0, 1.0);
        assert_eq!(d, vec![0.0, 0.0, 0.0, -1.0]);
        // C joins once D below 128.
        let d = overlap_sequence(&[228.0, 228.0, 228.0, 120.0], 228.0, 100.0, -1.0, 1.0);
        assert_eq!(d, vec![0.0, 0.0, -1.0, -1.0]);
        // Withdrawal: D moves only once C >= 128.
        let d = overlap_sequence(&[228.0, 228.0, 100.0, 0.0], 228.0, 100.0, 1.0, 1.0);
        assert_eq!(d, vec![0.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn boron_band_has_hysteresis() {
        let mut b = BoronControl::default();
        assert_eq!(b.update(160.0, 170.0, 215.0), 1, "rods deep: borate");
        assert_eq!(b.update(180.0, 170.0, 215.0), 1);
        assert_eq!(b.update(193.0, 170.0, 215.0), 0);
        assert_eq!(b.update(220.0, 170.0, 215.0), -1, "rods high: dilute");
    }
}
