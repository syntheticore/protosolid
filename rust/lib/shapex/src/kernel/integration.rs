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
    if (0..N)
        .all(|index| (left[index] + right[index] - whole[index]).abs() / 15.0 <= targets[index])
    {
        return Ok(std::array::from_fn(|index| {
            left[index] + right[index] + (left[index] + right[index] - whole[index]) / 15.0
        }));
    }
    if depth >= maximum_depth {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            failure_message,
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
