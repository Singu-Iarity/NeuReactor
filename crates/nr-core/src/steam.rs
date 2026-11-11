//! Water / steam properties.
//!
//! Saturation properties are interpolated from steam-table points
//! (IAPWS-IF97 reference values, rounded). Subcooled liquid density for the
//! PWR primary is tabulated at 15.5 MPa. A full IF97 implementation is on the
//! roadmap; these tables are accurate to well under 1% in the operating range.

use crate::math::Table1D;
use std::sync::OnceLock;

/// (p [MPa], Tsat [°C], hf [kJ/kg], hg [kJ/kg], rho_f [kg/m³], rho_g [kg/m³])
type SatRow = (f64, f64, f64, f64, f64, f64);
const SAT: &[SatRow] = &[
    (0.005, 32.88, 137.8, 2560.7, 994.7, 0.0354),
    (0.01, 45.81, 191.8, 2584.6, 989.8, 0.0682),
    (0.05, 81.32, 340.5, 2645.2, 970.9, 0.3086),
    (0.1, 99.61, 417.4, 2675.0, 958.6, 0.5903),
    (0.2, 120.21, 504.7, 2706.2, 942.9, 1.129),
    (0.5, 151.83, 640.1, 2748.1, 915.3, 2.668),
    (1.0, 179.88, 762.5, 2777.1, 887.1, 5.145),
    (2.0, 212.38, 908.5, 2798.4, 849.8, 10.04),
    (3.0, 233.86, 1008.3, 2803.2, 821.9, 15.01),
    (4.0, 250.36, 1087.4, 2800.8, 798.4, 20.09),
    (5.0, 263.94, 1154.5, 2794.2, 777.4, 25.35),
    (6.0, 275.59, 1213.8, 2784.6, 758.0, 30.82),
    (7.0, 285.83, 1267.7, 2772.6, 739.7, 36.53),
    (8.0, 295.01, 1317.3, 2758.7, 722.2, 42.51),
    (9.0, 303.35, 1363.7, 2742.9, 705.0, 48.79),
    (10.0, 311.00, 1407.9, 2725.5, 688.4, 55.47),
    (12.0, 324.68, 1491.5, 2685.4, 654.7, 70.06),
    (14.0, 336.67, 1571.6, 2638.1, 620.0, 87.10),
    (15.0, 342.16, 1610.3, 2610.8, 603.5, 96.71),
    (16.0, 347.36, 1650.5, 2580.8, 585.3, 107.4),
    (17.0, 352.29, 1691.7, 2547.0, 565.8, 119.5),
    (18.0, 356.99, 1734.8, 2509.5, 544.3, 133.0),
    (20.0, 365.75, 1826.6, 2410.4, 490.2, 170.7),
    (22.064, 373.95, 2084.3, 2084.3, 322.0, 322.0),
];

/// Compressed liquid at 15.5 MPa: (T [°C], rho [kg/m³]).
const LIQ_15_5: &[(f64, f64)] = &[
    (20.0, 1005.0),
    (100.0, 966.0),
    (150.0, 927.0),
    (200.0, 878.0),
    (250.0, 817.0),
    (270.0, 788.0),
    (280.0, 772.0),
    (290.0, 754.0),
    (300.0, 735.0),
    (310.0, 713.0),
    (320.0, 688.0),
    (330.0, 659.0),
    (340.0, 621.0),
    (344.8, 597.0),
];

struct Tables {
    tsat_of_p: Table1D,
    p_of_tsat: Table1D,
    hf: Table1D,
    hg: Table1D,
    rho_f: Table1D,
    rho_liq: Table1D,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let col = |f: fn(&SatRow) -> f64| -> Vec<(f64, f64)> {
            SAT.iter().map(|r| (r.0, f(r))).collect()
        };
        let tsat_of_p = Table1D::new(&col(|r| r.1));
        Tables {
            p_of_tsat: tsat_of_p.inverse(),
            tsat_of_p,
            hf: Table1D::new(&col(|r| r.2)),
            hg: Table1D::new(&col(|r| r.3)),
            rho_f: Table1D::new(&col(|r| r.4)),
            rho_liq: Table1D::new(LIQ_15_5),
        }
    })
}

/// Saturation temperature [°C] at pressure `p` [Pa].
pub fn tsat(p: f64) -> f64 {
    tables().tsat_of_p.eval(p / 1e6)
}

/// Saturation pressure [Pa] at temperature `t` [°C].
pub fn psat(t: f64) -> f64 {
    tables().p_of_tsat.eval(t) * 1e6
}

/// Saturated liquid enthalpy [J/kg] at pressure `p` [Pa].
pub fn hf(p: f64) -> f64 {
    tables().hf.eval(p / 1e6) * 1e3
}

/// Saturated vapour enthalpy [J/kg] at pressure `p` [Pa].
pub fn hg(p: f64) -> f64 {
    tables().hg.eval(p / 1e6) * 1e3
}

/// Saturated liquid density [kg/m³] at pressure `p` [Pa].
pub fn rho_f(p: f64) -> f64 {
    tables().rho_f.eval(p / 1e6)
}

/// Subcooled liquid density [kg/m³] at temperature `t` [°C], PWR primary pressure.
pub fn rho_primary(t: f64) -> f64 {
    tables().rho_liq.eval(t)
}

/// Approximate liquid enthalpy [J/kg] of compressed water at temperature `t` [°C].
pub fn h_liquid(t: f64) -> f64 {
    // Use saturated liquid enthalpy at the same temperature: pressure effect on
    // liquid enthalpy is small (<2%) in the ranges we use it.
    hf(psat(t.max(33.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturation_round_trip() {
        for &p in &[0.1e6, 1.0e6, 6.9e6, 15.5e6] {
            let t = tsat(p);
            assert!((psat(t) - p).abs() / p < 1e-9);
        }
        assert!((tsat(7.0e6) - 285.83).abs() < 1e-9);
        assert!((tsat(15.5e6) - 344.76).abs() < 0.1);
    }

    #[test]
    fn enthalpies_are_sane() {
        let p = 6.9e6;
        let hfg = hg(p) - hf(p);
        assert!((1.4e6..1.6e6).contains(&hfg));
        assert!(rho_primary(290.0) > rho_primary(325.0));
    }
}
