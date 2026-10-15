//! A number with its gradient over the ground, `(x, z)`: the carve's
//! arithmetic, so the gradient a carved sample carries is the derivative of
//! the height it carries, every term of the section (the discharge, the
//! cut, the order and the grain changing along a reach) included. Written
//! out by hand, the gradient held those fixed at the point and was off by
//! more than a tenth at one carved point in thirty (ticket 0005, the review
//! of step 2). Only correctly rounded operations: add, multiply, divide,
//! square root.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A value and its gradient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct D {
    pub v: f64,
    pub g: [f64; 2],
}

impl D {
    /// A value with a gradient.
    pub const fn new(v: f64, g: [f64; 2]) -> Self {
        Self { v, g }
    }

    /// A constant.
    pub const fn c(v: f64) -> Self {
        Self { v, g: [0.0, 0.0] }
    }

    /// The square root; a constant nought at or under nought, where the
    /// derivative is not finite.
    pub fn sqrt(self) -> Self {
        if self.v <= 0.0 {
            return Self::c(0.0);
        }
        let r = self.v.sqrt();
        let k = 0.5 / r;
        Self::new(r, [self.g[0] * k, self.g[1] * k])
    }

    /// The larger, with its gradient.
    pub fn max(self, other: Self) -> Self {
        if other.v > self.v { other } else { self }
    }

    /// The larger of this and a constant.
    pub fn at_least(self, floor: f64) -> Self {
        if self.v < floor { Self::c(floor) } else { self }
    }

    /// The smaller of this and a constant.
    pub fn at_most(self, ceiling: f64) -> Self {
        if self.v > ceiling {
            Self::c(ceiling)
        } else {
            self
        }
    }

    /// Times a constant.
    pub fn scale(self, k: f64) -> Self {
        Self::new(self.v * k, [self.g[0] * k, self.g[1] * k])
    }
}

impl Add for D {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.v + o.v, [self.g[0] + o.g[0], self.g[1] + o.g[1]])
    }
}

impl Add<f64> for D {
    type Output = Self;
    fn add(self, o: f64) -> Self {
        Self::new(self.v + o, self.g)
    }
}

impl Sub for D {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.v - o.v, [self.g[0] - o.g[0], self.g[1] - o.g[1]])
    }
}

impl Sub<f64> for D {
    type Output = Self;
    fn sub(self, o: f64) -> Self {
        Self::new(self.v - o, self.g)
    }
}

impl Sub<D> for f64 {
    type Output = D;
    fn sub(self, o: D) -> D {
        D::new(self - o.v, [-o.g[0], -o.g[1]])
    }
}

impl Mul for D {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::new(
            self.v * o.v,
            [
                self.g[0] * o.v + self.v * o.g[0],
                self.g[1] * o.v + self.v * o.g[1],
            ],
        )
    }
}

impl Mul<f64> for D {
    type Output = Self;
    fn mul(self, k: f64) -> Self {
        self.scale(k)
    }
}

impl Mul<D> for f64 {
    type Output = D;
    fn mul(self, o: D) -> D {
        o.scale(self)
    }
}

impl Div for D {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let inv = 1.0 / o.v;
        let v = self.v * inv;
        Self::new(
            v,
            [
                (self.g[0] - v * o.g[0]) * inv,
                (self.g[1] - v * o.g[1]) * inv,
            ],
        )
    }
}

impl Div<f64> for D {
    type Output = Self;
    fn div(self, k: f64) -> Self {
        self.scale(1.0 / k)
    }
}

impl Neg for D {
    type Output = Self;
    fn neg(self) -> Self {
        self.scale(-1.0)
    }
}

/// Smoothstep of `x` from `edge0` to `edge1`, edges and all with their
/// gradients; a step at `edge1` where the edges meet.
pub(crate) fn smooth(edge0: D, edge1: D, x: D) -> D {
    let span = edge1 - edge0;
    if span.v <= 0.0 {
        return D::c(if x.v >= edge1.v { 1.0 } else { 0.0 });
    }
    let t = (x - edge0) / span;
    if t.v <= 0.0 {
        D::c(0.0)
    } else if t.v >= 1.0 {
        D::c(1.0)
    } else {
        t * t * (3.0 - 2.0 * t)
    }
}

/// [`smooth`] between constant edges.
pub(crate) fn smooth_c(edge0: f64, edge1: f64, x: D) -> D {
    smooth(D::c(edge0), D::c(edge1), x)
}

/// The soft ramp of softness `k`: nought below `-k`, `x` above `k`, a
/// parabola between, so that `soft(x) - soft(-x) = x`.
pub(crate) fn soft(x: D, k: D) -> D {
    if k.v <= 0.0 {
        return if x.v > 0.0 { x } else { D::c(0.0) };
    }
    if x.v <= -k.v {
        D::c(0.0)
    } else if x.v >= k.v {
        x
    } else {
        let s = x + k;
        s * s / (k * 4.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A function of `(x, z)` through the dual arithmetic against central
    /// differences of its value.
    fn check(f: impl Fn(D, D) -> D, at: (f64, f64)) {
        let x = D::new(at.0, [1.0, 0.0]);
        let z = D::new(at.1, [0.0, 1.0]);
        let got = f(x, z);
        let e = 1e-6;
        let value = |a: f64, b: f64| f(D::c(a), D::c(b)).v;
        let gx = (value(at.0 + e, at.1) - value(at.0 - e, at.1)) / (2.0 * e);
        let gz = (value(at.0, at.1 + e) - value(at.0, at.1 - e)) / (2.0 * e);
        assert!((got.g[0] - gx).abs() < 1e-5, "{got:?} against {gx}");
        assert!((got.g[1] - gz).abs() < 1e-5, "{got:?} against {gz}");
    }

    #[test]
    fn gradients_agree_with_differences() {
        check(|x, z| x * z + (x / (z + 3.0)).sqrt(), (1.3, 0.7));
        check(|x, z| smooth(x, z * 3.0, x * z + 1.0), (0.4, 0.9));
        check(|x, z| soft(x - z, z * 0.5 + 0.1), (0.6, 0.5));
        check(
            |x, z| (x - z).max(z * 0.2).at_most(0.9).at_least(0.1),
            (0.8, 0.3),
        );
    }
}
