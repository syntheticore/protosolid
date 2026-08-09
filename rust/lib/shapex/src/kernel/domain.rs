use super::{DiagnosticKind, KernelError, KernelResult, ModelTolerance};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterRange {
    pub start: f64,
    pub end: f64,
}

impl ParameterRange {
    pub fn new(start: f64, end: f64) -> KernelResult<Self> {
        if !start.is_finite() || !end.is_finite() || start == end {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "a parameter range must have two distinct finite bounds",
            ));
        }
        Ok(Self { start, end })
    }

    pub fn span(self) -> f64 {
        self.end - self.start
    }

    pub fn reversed(self) -> Self {
        Self {
            start: self.end,
            end: self.start,
        }
    }

    pub fn contains(self, parameter: f64, tolerance: ModelTolerance) -> bool {
        let low = self.start.min(self.end) - tolerance.parameter;
        let high = self.start.max(self.end) + tolerance.parameter;
        low <= parameter && parameter <= high
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DomainBounds {
    Bounded(ParameterRange),
    LowerBounded(f64),
    UpperBounded(f64),
    Unbounded,
}

/// Natural domain metadata for a curve or one direction of a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterDomain {
    pub bounds: DomainBounds,
    pub period: Option<f64>,
}

impl ParameterDomain {
    pub fn bounded(start: f64, end: f64) -> KernelResult<Self> {
        Ok(Self {
            bounds: DomainBounds::Bounded(ParameterRange::new(start, end)?),
            period: None,
        })
    }

    pub fn periodic(start: f64, end: f64, period: f64) -> KernelResult<Self> {
        let range = ParameterRange::new(start, end)?;
        if !period.is_finite() || period <= 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "a periodic domain requires a finite positive period",
            ));
        }
        Ok(Self {
            bounds: DomainBounds::Bounded(range),
            period: Some(period),
        })
    }

    pub const fn unbounded() -> Self {
        Self {
            bounds: DomainBounds::Unbounded,
            period: None,
        }
    }

    pub fn normalize(self, parameter: f64) -> f64 {
        match self.period {
            Some(period) => {
                let origin = match self.bounds {
                    DomainBounds::Bounded(range) => range.start,
                    DomainBounds::LowerBounded(start) => start,
                    DomainBounds::UpperBounded(end) => end - period,
                    DomainBounds::Unbounded => 0.0,
                };
                origin + (parameter - origin).rem_euclid(period)
            }
            None => parameter,
        }
    }

    pub fn equivalent(self, first: f64, second: f64, tolerance: f64) -> bool {
        if (first - second).abs() <= tolerance {
            return true;
        }
        self.period
            .map(|period| {
                let difference = (first - second).rem_euclid(period);
                difference <= tolerance || (period - difference).abs() <= tolerance
            })
            .unwrap_or(false)
    }

    pub fn contains(self, parameter: f64, tolerance: f64) -> bool {
        if !parameter.is_finite() {
            return false;
        }
        if self.period.is_some() {
            return true;
        }
        match self.bounds {
            DomainBounds::Bounded(range) => {
                parameter >= range.start.min(range.end) - tolerance
                    && parameter <= range.start.max(range.end) + tolerance
            }
            DomainBounds::LowerBounded(start) => parameter >= start - tolerance,
            DomainBounds::UpperBounded(end) => parameter <= end + tolerance,
            DomainBounds::Unbounded => true,
        }
    }

    pub fn contains_range(self, range: ParameterRange, tolerance: f64) -> bool {
        self.contains(range.start, tolerance) && self.contains(range.end, tolerance)
    }
}
