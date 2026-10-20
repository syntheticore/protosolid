use cgmath::prelude::*;

use super::{DiagnosticKind, KernelError, KernelResult};

/// Scale-aware numerical policy for one model.
///
/// Linear values use model units. `relative` is multiplied by the operation's
/// characteristic length and combined with `linear` by taking the larger value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelTolerance {
    pub linear: f64,
    pub relative: f64,
    pub angular: f64,
    pub parameter: f64,
}

impl ModelTolerance {
    pub const fn new(linear: f64, relative: f64, angular: f64, parameter: f64) -> Self {
        Self {
            linear,
            relative,
            angular,
            parameter,
        }
    }

    pub fn validate(self) -> KernelResult<()> {
        let values = [self.linear, self.relative, self.angular, self.parameter];
        if values
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidTolerance,
                "all model tolerances must be finite and greater than zero",
            ));
        }
        if self.angular >= std::f64::consts::PI {
            return Err(KernelError::new(
                DiagnosticKind::InvalidTolerance,
                "angular tolerance must be smaller than pi radians",
            ));
        }
        Ok(())
    }

    pub fn linear_at_scale(self, scale: f64) -> f64 {
        self.linear.max(self.relative * scale.abs())
    }

    pub fn points_coincident(self, a: crate::Point3, b: crate::Point3, scale: f64) -> bool {
        a.distance2(b) <= self.linear_at_scale(scale).powi(2)
    }
}

impl Default for ModelTolerance {
    fn default() -> Self {
        Self::new(1.0e-7, 1.0e-12, 1.0e-10, 1.0e-12)
    }
}
