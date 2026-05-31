//! Small 2-D affine transform helper shared by classification and text-run
//! extraction.

use lopdf::Object;

/// A 2-D affine transform stored as the six PDF matrix components
/// `[a b c d e f]` (row-vector convention: `[x y 1] * M`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Matrix {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Matrix {
    pub(crate) const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Build a matrix from six numeric operands, if they are all numbers.
    pub(crate) fn from_operands(ops: &[Object]) -> Option<Matrix> {
        if ops.len() != 6 {
            return None;
        }
        let n = |i: usize| ops[i].as_float().ok();
        Some(Matrix {
            a: n(0)?,
            b: n(1)?,
            c: n(2)?,
            d: n(3)?,
            e: n(4)?,
            f: n(5)?,
        })
    }

    /// A pure translation.
    pub(crate) fn translation(tx: f32, ty: f32) -> Matrix {
        Matrix {
            e: tx,
            f: ty,
            ..Matrix::IDENTITY
        }
    }

    /// `self * other` (apply `self` first, then `other`).
    pub(crate) fn multiply(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// The six PDF matrix components `[a b c d e f]`.
    pub(crate) fn components(&self) -> [f32; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }

    /// Approximate vertical scale factor of the linear part.
    pub(crate) fn vertical_scale(&self) -> f32 {
        (self.c * self.c + self.d * self.d).sqrt()
    }

    /// Map a point `(x, y)` through the transform (row-vector convention).
    pub(crate) fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}
