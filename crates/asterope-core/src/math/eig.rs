use super::{abs, sqrt};

#[derive(Clone, Copy, Debug)]
pub struct SymEigen<const N: usize> {
    pub values: [f64; N],
    /// `vectors[row][k]` is component `row` of the eigenvector paired with `values[k]`.
    pub vectors: [[f64; N]; N],
}

impl<const N: usize> SymEigen<N> {
    pub fn max_index(&self) -> usize {
        let mut best = 0;
        let mut best_value = f64::NEG_INFINITY;
        for (k, v) in self.values.iter().enumerate() {
            if *v > best_value {
                best_value = *v;
                best = k;
            }
        }
        best
    }

    pub fn vector(&self, k: usize) -> [f64; N] {
        let mut out = [0.0; N];
        for (o, row) in out.iter_mut().zip(self.vectors.iter()) {
            *o = get1(row, k);
        }
        out
    }
}

fn get1<const N: usize>(row: &[f64; N], j: usize) -> f64 {
    row.get(j).copied().unwrap_or(0.0)
}

fn get<const N: usize>(m: &[[f64; N]; N], i: usize, j: usize) -> f64 {
    m.get(i).map_or(0.0, |row| get1(row, j))
}

fn set<const N: usize>(m: &mut [[f64; N]; N], i: usize, j: usize, v: f64) {
    if let Some(cell) = m.get_mut(i).and_then(|row| row.get_mut(j)) {
        *cell = v;
    }
}

/// Cyclic Jacobi eigenvalue iteration for a real symmetric matrix.
pub fn jacobi_eigen<const N: usize>(mut a: [[f64; N]; N]) -> SymEigen<N> {
    let mut v = [[0.0; N]; N];
    for i in 0..N {
        set(&mut v, i, i, 1.0);
    }

    for _sweep in 0..64 {
        let mut off = 0.0;
        for p in 0..N {
            for q in (p + 1)..N {
                let e = get(&a, p, q);
                off += e * e;
            }
        }
        if off == 0.0 {
            break;
        }
        for p in 0..N {
            for q in (p + 1)..N {
                let apq = get(&a, p, q);
                if apq == 0.0 {
                    continue;
                }
                let theta = (get(&a, q, q) - get(&a, p, p)) / (2.0 * apq);
                let sign = if theta >= 0.0 { 1.0 } else { -1.0 };
                let t = sign / (abs(theta) + sqrt(theta * theta + 1.0));
                let c = 1.0 / sqrt(t * t + 1.0);
                let s = t * c;
                for k in 0..N {
                    let akp = get(&a, k, p);
                    let akq = get(&a, k, q);
                    set(&mut a, k, p, c * akp - s * akq);
                    set(&mut a, k, q, s * akp + c * akq);
                }
                for k in 0..N {
                    let apk = get(&a, p, k);
                    let aqk = get(&a, q, k);
                    set(&mut a, p, k, c * apk - s * aqk);
                    set(&mut a, q, k, s * apk + c * aqk);
                }
                for k in 0..N {
                    let vkp = get(&v, k, p);
                    let vkq = get(&v, k, q);
                    set(&mut v, k, p, c * vkp - s * vkq);
                    set(&mut v, k, q, s * vkp + c * vkq);
                }
            }
        }
    }

    let mut values = [0.0; N];
    for (i, value) in values.iter_mut().enumerate() {
        *value = get(&a, i, i);
    }
    SymEigen { values, vectors: v }
}
