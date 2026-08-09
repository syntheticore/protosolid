use std::error::Error;
use std::fmt;

use super::PersistentId;

pub type KernelResult<T> = Result<T, KernelError>;

/// The class of invariant or algorithm failure reported by the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    InvalidInput,
    InvalidTolerance,
    MissingEntity,
    StaleHandle,
    TopologyInvariant,
    GeometryInvariant,
    NumericalFailure,
    UnsupportedGeometry,
    SerializationFailure,
}

/// A structured explanation of a failed operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    pub entities: Vec<PersistentId>,
    pub operation: Option<String>,
    pub required_tolerance: Option<f64>,
    pub achieved_error: Option<f64>,
}

impl Diagnostic {
    pub fn new(kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            entities: Vec::new(),
            operation: None,
            required_tolerance: None,
            achieved_error: None,
        }
    }

    pub fn with_entity(mut self, entity: PersistentId) -> Self {
        self.entities.push(entity);
        self
    }

    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    pub fn with_accuracy(mut self, required_tolerance: f64, achieved_error: f64) -> Self {
        self.required_tolerance = Some(required_tolerance);
        self.achieved_error = Some(achieved_error);
        self
    }
}

/// Errors are data, never panics. Public modeling operations use this type so a
/// caller can distinguish bad input, unsupported geometry, and kernel defects.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelError {
    diagnostics: Vec<Diagnostic>,
}

impl KernelError {
    pub fn new(kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            diagnostics: vec![Diagnostic::new(kind, message)],
        }
    }

    pub fn from_diagnostic(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    pub fn from_diagnostics(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index != 0 {
                f.write_str("; ")?;
            }
            write!(f, "{}", diagnostic.message)?;
            if let Some(operation) = &diagnostic.operation {
                write!(f, " [operation: {operation}]")?;
            }
            if let (Some(required), Some(achieved)) =
                (diagnostic.required_tolerance, diagnostic.achieved_error)
            {
                write!(f, " [required: {required:e}, achieved: {achieved:e}]")?;
            }
        }
        Ok(())
    }
}

impl Error for KernelError {}
