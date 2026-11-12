//! Generic numeric helpers.

/// Clamp `x` into `[lo, hi]`.
#[inline]
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

/// Linear interpolation between `a` and `b` by `t` (unclamped).
#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Piecewise-linear 1-D lookup table with monotonically increasing `x`.
/// Values outside the table range are clamped to the end points.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Table1D {
    xs: Vec<f64>,
    ys: Vec<f64>,
}

impl Table1D {
    pub fn new(points: &[(f64, f64)]) -> Self {
        assert!(points.len() >= 2, "table needs at least two points");
        for w in points.windows(2) {
            assert!(w[1].0 > w[0].0, "table x values must increase");
        }
        Self {
            xs: points.iter().map(|p| p.0).collect(),
            ys: points.iter().map(|p| p.1).collect(),
        }
    }

    /// Build the inverse table (y -> x). Requires `y` to be strictly increasing.
    pub fn inverse(&self) -> Self {
        let pts: Vec<(f64, f64)> = self
            .xs
            .iter()
            .zip(&self.ys)
            .map(|(x, y)| (*y, *x))
            .collect();
        Self::new(&pts)
    }

    pub fn eval(&self, x: f64) -> f64 {
        let n = self.xs.len();
        if x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }
        // Tables are small; binary search keeps lookups O(log n).
        let i = match self.xs.binary_search_by(|v| v.partial_cmp(&x).unwrap()) {
            Ok(i) => return self.ys[i],
            Err(i) => i - 1,
        };
        let t = (x - self.xs[i]) / (self.xs[i + 1] - self.xs[i]);
        lerp(self.ys[i], self.ys[i + 1], t)
    }
}

/// Proportional-integral controller with output clamping and anti-windup.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy)]
pub struct Pi {
    pub kp: f64,
    pub ki: f64,
    pub out_min: f64,
    pub out_max: f64,
    pub bias: f64,
    integral: f64,
}

impl Pi {
    pub fn new(kp: f64, ki: f64, out_min: f64, out_max: f64) -> Self {
        Self {
            kp,
            ki,
            out_min,
            out_max,
            bias: 0.0,
            integral: 0.0,
        }
    }

    pub fn with_bias(mut self, bias: f64) -> Self {
        self.bias = bias;
        self
    }

    /// Initialise the integral so that the output equals `output` at zero error
    /// (bumpless transfer from manual to automatic).
    pub fn track(&mut self, output: f64) {
        self.integral = output - self.bias;
    }

    pub fn update(&mut self, error: f64, dt: f64) -> f64 {
        let candidate = self.integral + self.ki * error * dt;
        let unclamped = self.bias + self.kp * error + candidate;
        let out = clamp(unclamped, self.out_min, self.out_max);
        // Only integrate when not saturated in the direction of the error.
        if (unclamped - out).abs() < 1e-12
            || (unclamped > self.out_max && error < 0.0)
            || (unclamped < self.out_min && error > 0.0)
        {
            self.integral = candidate;
        }
        out
    }
}

/// Limits how fast a value may change (units per second).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy)]
pub struct RateLimiter {
    pub rate: f64,
}

impl RateLimiter {
    pub fn step(&self, current: f64, target: f64, dt: f64) -> f64 {
        let max = self.rate * dt;
        current + clamp(target - current, -max, max)
    }
}

/// Deterministic xorshift64* RNG so simulations replay exactly from a seed.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal sample (Box-Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-300);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_interpolates_and_clamps() {
        let t = Table1D::new(&[(0.0, 0.0), (1.0, 10.0), (2.0, 30.0)]);
        assert_eq!(t.eval(-1.0), 0.0);
        assert_eq!(t.eval(0.5), 5.0);
        assert_eq!(t.eval(1.5), 20.0);
        assert_eq!(t.eval(5.0), 30.0);
        assert_eq!(t.inverse().eval(20.0), 1.5);
    }

    #[test]
    fn pi_converges_and_clamps() {
        // First-order plant y' = u - y, drive y to 0.7.
        let mut pi = Pi::new(2.0, 1.0, 0.0, 1.0);
        let mut y = 0.0;
        for _ in 0..20_000 {
            let u = pi.update(0.7 - y, 0.01);
            y += (u - y) * 0.01;
        }
        assert!((y - 0.7).abs() < 1e-3);
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mean: f64 = (0..10_000).map(|_| a.normal()).sum::<f64>() / 10_000.0;
        assert!(mean.abs() < 0.05);
    }
}
