use super::{DiagnosticKind, KernelError, KernelResult};

fn simpson<const N: usize>(
    start: f64,
    end: f64,
    first: [f64; N],
    middle: [f64; N],
    last: [f64; N],
) -> [f64; N] {
    std::array::from_fn(|index| {
        (end - start) * (first[index] + 4.0 * middle[index] + last[index]) / 6.0
    })
}

pub(crate) fn adaptive_simpson<const N: usize>(
    start: f64,
    end: f64,
    terms: impl Fn(f64) -> KernelResult<[f64; N]>,
    targets: [f64; N],
    maximum_depth: u8,
    failure_message: &'static str,
) -> KernelResult<[f64; N]> {
    let middle = (start + end) * 0.5;
    let first = terms(start)?;
    let center = terms(middle)?;
    let last = terms(end)?;
    let whole = simpson(start, end, first, center, last);
    adaptive_interval(
        &terms,
        start,
        end,
        first,
        center,
        last,
        whole,
        targets,
        0,
        maximum_depth,
        failure_message,
    )
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
    gauss_kronrod_interval(
        &terms,
        start,
        end,
        targets,
        0,
        maximum_depth,
        failure_message,
    )
}

fn gauss_kronrod_interval<const N: usize, F>(
    terms: &F,
    start: f64,
    end: f64,
    targets: [f64; N],
    depth: u8,
    maximum_depth: u8,
    failure_message: &'static str,
) -> KernelResult<[f64; N]>
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
    if (0..N).all(|index| errors[index] <= targets[index]) {
        return Ok(kronrod);
    }
    if depth >= maximum_depth {
        let worst = (0..N)
            .max_by(|first, second| {
                (errors[*first] / targets[*first]).total_cmp(&(errors[*second] / targets[*second]))
            })
            .unwrap_or(0);
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            format!(
                "{failure_message} (term {worst}, interval [{start}, {end}], estimated error {}, target {})",
                errors[worst], targets[worst],
            ),
        ));
    }
    let middle = (start + end) * 0.5;
    let child_targets = targets.map(|target| target * 0.5);
    let first = gauss_kronrod_interval(
        terms,
        start,
        middle,
        child_targets,
        depth + 1,
        maximum_depth,
        failure_message,
    )?;
    let second = gauss_kronrod_interval(
        terms,
        middle,
        end,
        child_targets,
        depth + 1,
        maximum_depth,
        failure_message,
    )?;
    Ok(std::array::from_fn(|index| first[index] + second[index]))
}

#[allow(clippy::too_many_arguments)]
fn adaptive_interval<const N: usize, F>(
    terms: &F,
    start: f64,
    end: f64,
    first: [f64; N],
    middle: [f64; N],
    last: [f64; N],
    whole: [f64; N],
    targets: [f64; N],
    depth: u8,
    maximum_depth: u8,
    failure_message: &'static str,
) -> KernelResult<[f64; N]>
where
    F: Fn(f64) -> KernelResult<[f64; N]>,
{
    let center = (start + end) * 0.5;
    let left_center = (start + center) * 0.5;
    let right_center = (center + end) * 0.5;
    let left_value = terms(left_center)?;
    let right_value = terms(right_center)?;
    let left = simpson(start, center, first, left_value, middle);
    let right = simpson(center, end, middle, right_value, last);
    let errors = std::array::from_fn::<_, N, _>(|index| {
        (left[index] + right[index] - whole[index]).abs() / 15.0
    });
    if (0..N).all(|index| errors[index] <= targets[index]) {
        return Ok(std::array::from_fn(|index| {
            left[index] + right[index] + (left[index] + right[index] - whole[index]) / 15.0
        }));
    }
    if depth >= maximum_depth {
        let worst = (0..N)
            .max_by(|first, second| {
                (errors[*first] / targets[*first]).total_cmp(&(errors[*second] / targets[*second]))
            })
            .unwrap_or(0);
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            format!(
                "{failure_message} (term {worst}, interval [{start}, {end}], estimated error {}, target {})",
                errors[worst], targets[worst],
            ),
        ));
    }
    let child_targets = targets.map(|target| target * 0.5);
    let first_half = adaptive_interval(
        terms,
        start,
        center,
        first,
        left_value,
        middle,
        left,
        child_targets,
        depth + 1,
        maximum_depth,
        failure_message,
    )?;
    let second_half = adaptive_interval(
        terms,
        center,
        end,
        middle,
        right_value,
        last,
        right,
        child_targets,
        depth + 1,
        maximum_depth,
        failure_message,
    )?;
    Ok(std::array::from_fn(|index| {
        first_half[index] + second_half[index]
    }))
}
