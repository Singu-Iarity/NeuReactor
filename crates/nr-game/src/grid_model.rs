//! System (network-wide) electricity demand, as a fraction of the annual
//! peak, and what it means for this unit's dispatch.
//!
//! Demand follows the usual patterns: an overnight trough (~04:00), a
//! morning rise, a daytime plateau and an early-evening peak; quieter
//! weekends; winter and summer peaks with spring / autumn lows. The grid
//! operator uses it to decide how far your unit may come down (derates are
//! refused at peak), how much extra output it will take, and how big a
//! base-load block the system can absorb through every overnight and
//! weekend trough in the period.
//!
//! Calendar time is seconds since 00:00 on 1 January (a Monday).

use std::f64::consts::TAU;

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Day of year (0-364), hour of day (0-24) and day of week (0 Monday).
pub fn calendar(cal_s: f64) -> (f64, f64, i64) {
    let day = cal_s / 86_400.0;
    let doy = day.rem_euclid(365.0);
    let hour = (cal_s / 3600.0).rem_euclid(24.0);
    let dow = (day.floor() as i64).rem_euclid(7);
    (doy, hour, dow)
}

/// System demand as a fraction of the annual peak (about 0.5 to 1.0).
pub fn system_load(cal_s: f64) -> f64 {
    let (doy, hour, dow) = calendar(cal_s);
    // Seasons: peaks mid-January and mid-July, winter a little higher.
    let season = 0.84
        + 0.08 * (TAU * (doy - 20.0) / 182.5).cos()
        + 0.03 * (TAU * (doy - 20.0) / 365.0).cos();
    // Day: trough overnight, morning rise, plateau, evening peak.
    let daily = 0.78 + 0.14 * smoothstep(5.0, 9.0, hour) - 0.14 * smoothstep(21.0, 24.5, hour)
        + 0.08 * (-((hour - 18.5) / 1.6).powi(2)).exp();
    let weekly = match dow {
        5 => 0.93,
        6 => 0.89,
        _ => 1.0,
    };
    // Weather / random variation (deterministic per hour).
    let k = (cal_s / 3600.0).floor();
    let noise = 0.015 * ((k * 12.9898).sin() * 43_758.545).fract();
    (season * daily * weekly + noise).clamp(0.45, 1.0)
}

/// Wind / solar availability for a day (0 calm .. 1 very windy),
/// deterministic per calendar day.
fn wind(cal_s: f64) -> f64 {
    let d = (cal_s / 86_400.0).floor();
    ((d * 78.233).sin() * 43_758.545).fract().abs()
}

/// Wholesale price [$/MWh at a $45 average] from system load: a merit
/// order (cheap plants at the base, expensive peakers at the top) less a
/// renewable surplus on windy days, which can push quiet nights negative.
fn merit_order(load: f64, wind: f64) -> f64 {
    let base = 12.0 + 100.0 * (load - 0.5) + 2500.0 * (load - 0.88).max(0.0).powi(2);
    let surplus = if load < 0.62 { 18.0 } else { 6.0 };
    base - surplus * wind.powi(2)
}

/// Average of the merit-order price over a year (normalisation).
const MARKET_MEAN: f64 = 31.7;

/// Day-ahead price relative to the average market price (forecast: no
/// real-time noise).
pub fn day_ahead_factor(cal_s: f64) -> f64 {
    let load = system_load(cal_s);
    merit_order(load, wind(cal_s)) / MARKET_MEAN
}

/// Real-time price relative to the average: day-ahead plus balancing
/// noise (deterministic per 5-minute interval).
pub fn real_time_factor(cal_s: f64) -> f64 {
    let k = (cal_s / 300.0).floor();
    let noise = (((k * 39.346).sin() * 43_758.545).fract() - 0.5) * 0.12;
    day_ahead_factor(cal_s) + noise
}

/// Lowest, average and highest system load over the next `hours`.
pub fn load_window(cal_s: f64, hours: f64) -> (f64, f64, f64) {
    let n = (hours.max(1.0) * 2.0).ceil() as usize;
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    let mut sum = 0.0;
    for i in 0..=n {
        let l = system_load(cal_s + i as f64 * 1800.0);
        lo = lo.min(l);
        hi = hi.max(l);
        sum += l;
    }
    (lo, sum / (n + 1) as f64, hi)
}

/// Plain-language state of the network.
pub fn state(load: f64) -> &'static str {
    if load >= 0.92 {
        "PEAK: CONSERVATIVE OPERATIONS"
    } else if load >= 0.82 {
        "TIGHT"
    } else if load >= 0.65 {
        "NORMAL"
    } else {
        "LONG (SURPLUS)"
    }
}

/// Lowest output (fraction of the unit's capability) the grid will let
/// the unit come down to over the next few hours.
pub fn derate_floor(peak: f64) -> f64 {
    0.25 + 0.75 * ((peak - 0.60) / 0.33).clamp(0.0, 1.0)
}

/// Highest output (fraction of capability) the grid will take.
pub fn raise_ceiling(avg: f64) -> f64 {
    0.5 + 0.5 * ((avg - 0.60) / 0.30).clamp(0.0, 1.0)
}

/// Largest base-load block (fraction of capability) the system can absorb
/// through the lowest demand of the period.
pub fn baseload_cap(min_load: f64) -> f64 {
    0.6 + 0.4 * ((min_load - 0.50) / 0.30).clamp(0.0, 1.0)
}

/// Dispatch target for load-following requests (fraction of capability).
pub fn dispatch_fraction(load: f64) -> f64 {
    (0.45 + 0.55 * ((load - 0.55) / 0.40)).clamp(0.4, 1.0)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// "Tue 14 Jan 18:40"
pub fn label(cal_s: f64) -> String {
    let (doy, hour, dow) = calendar(cal_s);
    let lens = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut d = doy.floor() as i64;
    let mut m = 0;
    while m < 11 && d >= lens[m] {
        d -= lens[m];
        m += 1;
    }
    format!(
        "{} {} {} {:02}:{:02}",
        DAYS[dow as usize],
        d + 1,
        MONTHS[m],
        hour.floor() as i64,
        ((hour.fract()) * 60.0).floor() as i64
    )
}

/// Season name for messages.
pub fn season(cal_s: f64) -> &'static str {
    let (doy, _, _) = calendar(cal_s);
    match doy as i64 {
        0..=58 | 335..=365 => "winter",
        59..=150 => "spring",
        151..=242 => "summer",
        _ => "autumn",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f64 = 3600.0;
    const D: f64 = 86_400.0;

    #[test]
    fn evening_peak_beats_night_and_winter_beats_spring() {
        let jan_tue = D + 18.5 * H; // Tue 2 Jan 18:30
        let jan_tue_night = D + 4.0 * H;
        assert!(system_load(jan_tue) > system_load(jan_tue_night) + 0.15);
        let apr_tue = (91.0 + 1.0) * D + 18.5 * H;
        assert!(system_load(jan_tue) > system_load(apr_tue) + 0.05);
        let jan_sun = 6.0 * D + 18.5 * H;
        assert!(system_load(jan_tue) > system_load(jan_sun));
        assert_eq!(state(system_load(jan_tue)), "PEAK: CONSERVATIVE OPERATIONS");
    }

    #[test]
    fn longer_blocks_and_spring_mean_smaller_baseload() {
        let jan = D + 8.0 * H;
        let apr = 91.0 * D + 8.0 * H;
        let cap = |t: f64, h: f64| baseload_cap(load_window(t, h).0);
        assert!(cap(jan, 24.0) >= cap(jan, 168.0));
        assert!(cap(jan, 168.0) > cap(apr, 168.0));
        assert!(cap(apr, 168.0) >= 0.6);
    }

    #[test]
    fn calendar_label() {
        assert_eq!(label(D + 18.5 * H), "Tue 2 Jan 18:30");
        assert_eq!(label(31.0 * D), "Thu 1 Feb 00:00");
    }

    #[test]
    fn market_price_follows_demand_and_averages_out() {
        // Year-round average close to 1 (the base price).
        let n = 365 * 24;
        let avg: f64 = (0..n).map(|h| day_ahead_factor(h as f64 * H)).sum::<f64>() / n as f64;
        assert!((avg - 1.0).abs() < 0.08, "average price factor {avg}");
        // Winter evening peak is dear, spring night cheap.
        assert!(day_ahead_factor(D + 18.5 * H) > 1.3);
        assert!(day_ahead_factor(97.0 * D + 4.0 * H) < 0.7);
        // Some hours in the year go negative (windy quiet nights).
        assert!((0..n).any(|h| day_ahead_factor(h as f64 * H) < 0.0));
    }
}
