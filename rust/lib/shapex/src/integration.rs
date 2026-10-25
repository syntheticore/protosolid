use super::{DiagnosticKind, KernelError, KernelResult};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadrature_resolves_a_parameter_break_in_both_directions() {
        let knot = 0.315791;
        let terms = |parameter| Ok([if parameter < knot { 1.0 } else { 3.0 }, parameter]);
        let targets = [1.0e-12; 2];
        assert!(adaptive_gauss_kronrod(0.0, 1.0, terms, targets, 2, "not converged").is_err());
        for (start, end, sign) in [(0.0, 1.0, 1.0), (1.0, 0.0, -1.0)] {
            let result = adaptive_gauss_kronrod_partitioned(
                start,
                end,
                terms,
                targets,
                2,
                "not converged",
                &[knot],
            )
            .unwrap();
            assert!((result[0] - sign * (3.0 - 2.0 * knot)).abs() < targets[0]);
            assert!((result[1] - sign * 0.5).abs() < targets[1]);
        }
    }
}

/// Adaptive embedded Gauss-Kronrod 15/7 integration.
///
/// Both rules are open: no evaluation is made at an interval endpoint. This is
/// required for exact trimmed BREP boundaries because an implicit intersection
/// may have a unique one-sided tangent on the trim while its untrimmed support
/// has no unique tangent at the endpoint itself. The Kronrod/Gauss difference
/// supplies the local error estimate without assigning a value at that
/// measure-zero singularity.
pub(crate) fn adaptive_gauss_kronrod<const N: usize>(
    start: f64,
    end: f64,
    terms: impl Fn(f64) -> KernelResult<[f64; N]>,
    targets: [f64; N],
    maximum_depth: u8,
    failure_message: &'static str,
) -> KernelResult<[f64; N]> {
    adaptive_gauss_kronrod_partitioned(
        start,
        end,
        terms,
        targets,
        maximum_depth,
        failure_message,
        &[],
    )
}

/// Integrate each known smooth parameter span separately. A short span can
/// otherwise fall between all quadrature nodes and escape the error estimate.
pub(crate) fn adaptive_gauss_kronrod_partitioned<const N: usize>(
    start: f64,
    end: f64,
    terms: impl Fn(f64) -> KernelResult<[f64; N]>,
    targets: [f64; N],
    maximum_depth: u8,
    failure_message: &'static str,
    breakpoints: &[f64],
) -> KernelResult<[f64; N]> {
    adaptive_partitioned(start, end, |a, b| gauss_kronrod_estimate(&terms, a, b),
        targets, maximum_depth, failure_message, breakpoints)
}

pub(crate) fn adaptive_partitioned<const N: usize>(
    start: f64, end: f64,
    estimate: impl Fn(f64, f64) -> KernelResult<([f64; N], [f64; N])>,
    targets: [f64; N], maximum_depth: u8, failure_message: &'static str,
    breakpoints: &[f64],
) -> KernelResult<[f64; N]> {
    let mut parameters = vec![start, end];
    parameters.extend(breakpoints.iter().copied().filter(|point| {
        *point > start.min(end) && *point < start.max(end)
    }));
    parameters.sort_by(f64::total_cmp);
    parameters.dedup();
    if end < start { parameters.reverse(); }
    let mut intervals = Vec::new();
    for pair in parameters.windows(2) {
        let (values, errors) = estimate(pair[0], pair[1])?;
        intervals.push((pair[0], pair[1], values, errors, 0));
    }
    loop {
        let mut total = [0.0; N];
        let mut errors = [0.0; N];
        for (_, _, values, local_errors, _) in &intervals {
            for index in 0..N {
                total[index] += values[index];
                errors[index] += local_errors[index];
            }
        }
        if (0..N).all(|index| errors[index] <= targets[index]) {
            return Ok(total);
        }
        // Refine the largest contributor to the remaining total error. Tiny
        // knot spans need not meet an arbitrarily tiny fraction of the budget
        // when the other spans have already converged much more accurately.
        let score = |local_errors: &[f64; N]| {
            (0..N).filter(|index| errors[*index] > targets[*index])
                .map(|index| local_errors[index] / targets[index])
                .fold(0.0_f64, f64::max)
        };
        let index = (0..intervals.len()).max_by(|a, b| {
            score(&intervals[*a].3).total_cmp(&score(&intervals[*b].3))
        }).unwrap();
        let (start, end, _, local_errors, depth) = intervals.swap_remove(index);
        if depth >= maximum_depth {
            let worst = (0..N).max_by(|a, b| {
                (local_errors[*a] / targets[*a]).total_cmp(&(local_errors[*b] / targets[*b]))
            }).unwrap();
            return Err(KernelError::new(DiagnosticKind::NumericalFailure, format!(
                "{failure_message} (term {worst}, interval [{start}, {end}], estimated error {}, target {})",
                errors[worst], targets[worst],
            )));
        }
        let middle = (start + end) * 0.5;
        for (start, end) in [(start, middle), (middle, end)] {
            let (values, errors) = estimate(start, end)?;
            intervals.push((start, end, values, errors, depth + 1));
        }
    }
}

fn gauss_kronrod_estimate<const N: usize, F>(
    terms: &F,
    start: f64,
    end: f64,
) -> KernelResult<([f64; N], [f64; N])>
where
    F: Fn(f64) -> KernelResult<[f64; N]>,
{
    // Positive abscissae of the Kronrod extension, ordered outside-in, plus
    // the central node. Odd indices are the embedded seven-point Gauss rule.
    const NODES: [f64; 8] = [
        0.991_455_371_120_812_6,
        0.949_107_912_342_758_5,
        0.864_864_423_359_769_1,
        0.741_531_185_599_394_5,
        0.586_087_235_467_691_1,
        0.405_845_151_377_397_2,
        0.207_784_955_007_898_48,
        0.0,
    ];
    const KRONROD_WEIGHTS: [f64; 8] = [
        0.022_935_322_010_529_224,
        0.063_092_092_629_978_56,
        0.104_790_010_322_250_19,
        0.140_653_259_715_525_92,
        0.169_004_726_639_267_9,
        0.190_350_578_064_785_42,
        0.204_432_940_075_298_89,
        0.209_482_141_084_727_82,
    ];
    const GAUSS_WEIGHTS: [f64; 4] = [
        0.129_484_966_168_869_7,
        0.279_705_391_489_276_64,
        0.381_830_050_505_118_9,
        0.417_959_183_673_469_4,
    ];

    let center = (start + end) * 0.5;
    let half_span = (end - start) * 0.5;
    let center_value = terms(center)?;
    let mut kronrod = center_value.map(|value| value * KRONROD_WEIGHTS[7]);
    let mut gauss = center_value.map(|value| value * GAUSS_WEIGHTS[3]);
    for index in 0..7 {
        let offset = half_span * NODES[index];
        let first = terms(center - offset)?;
        let second = terms(center + offset)?;
        for term in 0..N {
            let pair = first[term] + second[term];
            kronrod[term] += pair * KRONROD_WEIGHTS[index];
            if index % 2 == 1 {
                gauss[term] += pair * GAUSS_WEIGHTS[index / 2];
            }
        }
    }
    kronrod = kronrod.map(|value| value * half_span);
    gauss = gauss.map(|value| value * half_span);
    let errors = std::array::from_fn::<_, N, _>(|index| (kronrod[index] - gauss[index]).abs());
    Ok((kronrod, errors))
}
