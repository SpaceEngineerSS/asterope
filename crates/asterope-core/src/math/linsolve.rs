use super::abs;

fn get<const N: usize>(m: &[[f64; N]; N], i: usize, j: usize) -> f64 {
    m.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0.0)
}

/// Solves `a x = b` by Gaussian elimination with partial pivoting.
pub fn solve_linear<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let mut pivot = col;
        let mut pivot_mag = abs(get(&a, col, col));
        for row in (col + 1)..N {
            let mag = abs(get(&a, row, col));
            if mag > pivot_mag {
                pivot_mag = mag;
                pivot = row;
            }
        }
        if pivot_mag == 0.0 || !pivot_mag.is_finite() {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);

        let pivot_row = *a.get(col)?;
        let pivot_value = *pivot_row.get(col)?;
        let pivot_rhs = *b.get(col)?;
        for row in (col + 1)..N {
            let factor = get(&a, row, col) / pivot_value;
            if factor == 0.0 {
                continue;
            }
            if let Some(target) = a.get_mut(row) {
                for (t, p) in target.iter_mut().zip(pivot_row.iter()).skip(col) {
                    *t -= factor * p;
                }
            }
            if let Some(rhs) = b.get_mut(row) {
                *rhs -= factor * pivot_rhs;
            }
        }
    }

    let mut x = [0.0; N];
    for row in (0..N).rev() {
        let mut acc = *b.get(row)?;
        for col in (row + 1)..N {
            acc -= get(&a, row, col) * x.get(col).copied().unwrap_or(0.0);
        }
        let diag = get(&a, row, row);
        if diag == 0.0 {
            return None;
        }
        *x.get_mut(row)? = acc / diag;
    }
    if x.iter().all(|v| v.is_finite()) {
        Some(x)
    } else {
        None
    }
}
