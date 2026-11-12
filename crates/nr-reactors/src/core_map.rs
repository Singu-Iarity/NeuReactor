//! Core layout and core exit thermocouples (CETs).
//!
//! A four-loop PWR core is 193 fuel assemblies on a 15×15 grid with 53 rod
//! cluster control assemblies (RCCAs). Fuel rod temperatures themselves are
//! not instrumented in a PWR; what operators see is the coolant temperature
//! leaving each assembly, measured by thermocouples above the core. These
//! follow the radial power shape: hotter in the centre, colder under
//! inserted rods, and tilted by quadrant when rod groups are misaligned.
//!
//! The layout here matches `web/js/core.js` (same row widths, same RCCA
//! orbits, same row-major order of in-core cells).

use nr_physics::RodBank;

pub const N: usize = 15;
const C: i32 = 7;
const ROW_WIDTH: [i32; N] = [7, 11, 13, 13, 15, 15, 15, 15, 15, 15, 15, 13, 13, 11, 7];

/// RCCA orbits: (dx, dy) from the core centre → bank.
const ORBITS: [((i32, i32), &str); 10] = [
    ((0, 0), "D"),
    ((2, 2), "D"),
    ((2, 0), "C"),
    ((4, 4), "C"),
    ((4, 0), "B"),
    ((6, 0), "B"),
    ((4, 2), "A"),
    ((6, 2), "SA"),
    ((3, 1), "SA"),
    ((6, 4), "SB"),
];

#[derive(Debug, Clone)]
pub struct Assembly {
    /// Column / row on the 15×15 grid (row 0 at the top).
    pub x: i32,
    pub y: i32,
    /// Quadrant 0..3 (N41..N44 convention: 1 = +x+y, 2 = −x+y, 3 = −x−y, 4 = +x−y).
    pub quadrant: usize,
    /// RCCA in this assembly: bank name and group (1 or 2).
    pub rcca: Option<(&'static str, u8)>,
    /// Relative radial power with all rods out (mean 1).
    pub shape: f64,
    /// Fixed thermocouple calibration offset [°C].
    pub tc_offset: f64,
}

fn in_core(x: i32, y: i32) -> bool {
    (x - C).abs() <= (ROW_WIDTH[y as usize] - 1) / 2
}

fn rcca_at(dx: i32, dy: i32) -> Option<(&'static str, u8)> {
    for ((a, b), bank) in ORBITS {
        let hit = [(a, b), (b, a)]
            .iter()
            .any(|&(p, q)| dx.abs() == p && dy.abs() == q);
        if hit {
            let group = if dx == 0 {
                1
            } else if dy == 0 || dx * dy < 0 {
                2
            } else {
                1
            };
            return Some((bank, group));
        }
    }
    None
}

/// The core layout (built once).
pub fn core() -> &'static [Assembly] {
    static CORE: std::sync::OnceLock<Vec<Assembly>> = std::sync::OnceLock::new();
    CORE.get_or_init(assemblies)
}

/// All in-core assemblies, row-major.
pub fn assemblies() -> Vec<Assembly> {
    let mut out = Vec::with_capacity(193);
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            if !in_core(x, y) {
                continue;
            }
            let dx = x - C;
            let dy = C - y;
            let r = ((dx * dx + dy * dy) as f64).sqrt();
            // Bessel-like radial shape, flattened by low-leakage loading.
            let shape = 0.5 + 0.5 * (std::f64::consts::FRAC_PI_2 * r / 9.6).cos();
            let quadrant = match (dx >= 0, dy >= 0) {
                (true, true) => 0,
                (false, true) => 1,
                (false, false) => 2,
                (true, false) => 3,
            };
            // Deterministic ±0.4 °C thermocouple calibration scatter.
            let h = ((x * 73 + y * 151) % 17) as f64 / 16.0 - 0.5;
            out.push(Assembly {
                x,
                y,
                quadrant,
                rcca: rcca_at(dx, dy),
                shape,
                tc_offset: 0.8 * h,
            });
        }
    }
    let mean = out.iter().map(|a| a.shape).sum::<f64>() / out.len() as f64;
    for a in &mut out {
        a.shape /= mean;
    }
    out
}

/// Relative power of each assembly for the current rod pattern and tilt
/// (mean 1).
pub fn assembly_power(core: &[Assembly], banks: &[RodBank], tilt: [f64; 4]) -> Vec<f64> {
    let mut p: Vec<f64> = core
        .iter()
        .map(|a| {
            let mut f = a.shape * (1.0 + tilt[a.quadrant]);
            if let Some((bank, group)) = a.rcca {
                if let Some(b) = banks.iter().find(|b| b.name == bank) {
                    let pos = if group == 2 {
                        b.position + b.group2_offset
                    } else {
                        b.position
                    };
                    let ins = 1.0 - (pos / b.max_steps).clamp(0.0, 1.0);
                    f *= 1.0 - 0.45 * ins;
                }
            }
            f
        })
        .collect();
    let mean = p.iter().sum::<f64>() / p.len() as f64;
    if mean > 0.0 {
        for v in &mut p {
            *v /= mean;
        }
    }
    p
}

/// Core exit temperature of each assembly [°C].
pub fn exit_temps(core: &[Assembly], power: &[f64], t_cold: f64, t_hot: f64) -> Vec<f64> {
    let dt = t_hot - t_cold;
    core.iter()
        .zip(power)
        .map(|(a, f)| t_cold + dt * f + a.tc_offset)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_a_four_loop_core() {
        let core = assemblies();
        assert_eq!(core.len(), 193);
        assert_eq!(core.iter().filter(|a| a.rcca.is_some()).count(), 53);
        for bank in ["A", "B", "C", "D", "SA", "SB"] {
            assert!(core.iter().any(|a| a.rcca.map(|r| r.0) == Some(bank)));
        }
    }

    #[test]
    fn inserted_rods_cool_their_assemblies() {
        let core = assemblies();
        let mut banks = vec![RodBank::new("D", 228.0, 0.01, 228.0, false)];
        let out = assembly_power(&core, &banks, [0.0; 4]);
        banks[0].position = 0.0;
        let ins = assembly_power(&core, &banks, [0.0; 4]);
        let centre = core.iter().position(|a| a.x == 7 && a.y == 7).unwrap();
        assert!(ins[centre] < out[centre] * 0.8);
        let mean = ins.iter().sum::<f64>() / ins.len() as f64;
        assert!((mean - 1.0).abs() < 1e-9);
    }
}
