// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module defines the [`Error`] struct and the [`ErrorKind`] enum, which
//! represent errors that can occur in the library, along with
//! [`ValidationError`] for the individual failures collected while validating a
//! graph.

/// The kind of an [`Error`].
///
/// Marked `#[non_exhaustive]`: matching on this enum from outside the crate
/// must include a wildcard arm, so future kinds can be added without a
/// breaking change.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No component was found for a given component ID.
    ComponentNotFound,

    /// An internal invariant of the library was violated. This indicates a bug.
    Internal,

    /// A component is invalid, e.g. it has an unspecified category.
    InvalidComponent,

    /// A connection between two components is invalid.
    InvalidConnection,

    /// The graph is structurally invalid, e.g. it has no grid component,
    /// several grid components, or a duplicate component ID.
    InvalidGraph,

    /// One or more checks failed while validating an otherwise well-formed
    /// graph. Holds every [`ValidationError`] that was collected.
    ValidationErrors(Vec<ValidationError>),
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::ComponentNotFound => "ComponentNotFound",
            Self::Internal => "Internal",
            Self::InvalidComponent => "InvalidComponent",
            Self::InvalidConnection => "InvalidConnection",
            Self::InvalidGraph => "InvalidGraph",
            Self::ValidationErrors(_) => "ValidationErrors",
        };
        f.write_str(name)
    }
}

/// An error that can occur during the creation or traversal of a
/// [ComponentGraph][crate::ComponentGraph].
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    desc: String,
}

impl Error {
    /// Returns the [`ErrorKind`] of this error.
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// If this is an [`ErrorKind::ValidationErrors`], returns the collected
    /// failures; otherwise returns the error unchanged. Any other kind arising
    /// during validation signals an internal inconsistency rather than a
    /// topology failure (e.g. a graph lookup returning `ComponentNotFound`), so
    /// it is propagated to abort validation rather than collected.
    pub(crate) fn into_validation_errors(self) -> Result<Vec<ValidationError>, Error> {
        match self.kind {
            ErrorKind::ValidationErrors(errors) => Ok(errors),
            kind => Err(Error {
                kind,
                desc: self.desc,
            }),
        }
    }
}

/// Constructors for [`Error`].
impl Error {
    /// Creates a new [`Error`] with the `ComponentNotFound` kind and the given
    /// description.
    pub(crate) fn component_not_found(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::ComponentNotFound,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `Internal` kind and the given
    /// description.
    pub(crate) fn internal(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Internal,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `InvalidComponent` kind and the given
    /// description.
    pub(crate) fn invalid_component(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidComponent,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `InvalidConnection` kind and the given
    /// description.
    pub(crate) fn invalid_connection(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidConnection,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `InvalidGraph` kind and the given
    /// description.
    pub(crate) fn invalid_graph(desc: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidGraph,
            desc: desc.into(),
        }
    }

    /// Creates a new [`Error`] with the `ValidationErrors` kind from the
    /// collected validation failures.
    pub(crate) fn validation_errors(errors: Vec<ValidationError>) -> Self {
        // The failures carry their own descriptions, so `desc` is unused for
        // this kind; `Display` formats the collected errors directly.
        Self {
            kind: ErrorKind::ValidationErrors(errors),
            desc: String::new(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ErrorKind::ValidationErrors(errors) => {
                write!(f, "Graph validation failed:")?;
                for error in errors {
                    write!(f, "\n    {error}")?;
                }
                Ok(())
            }
            kind => write!(f, "{kind}: {}", self.desc),
        }
    }
}

impl std::error::Error for Error {}

/// A single failure found while validating a
/// [`ComponentGraph`][crate::ComponentGraph]'s topology.
///
/// Validation collects every failure rather than stopping at the first one, so
/// a single graph can yield many of these (see [`ErrorKind::ValidationErrors`]).
/// Besides a human-readable [`message`][Self::message], each failure exposes the
/// [`component_ids`][Self::component_ids] it involves, so callers can act on the
/// affected components without parsing the message text.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    message: String,
    component_ids: Vec<u64>,
}

impl ValidationError {
    /// Creates a new validation error from a message and the IDs of the
    /// components it involves.
    pub(crate) fn new(message: impl Into<String>, component_ids: impl Into<Vec<u64>>) -> Self {
        Self {
            message: message.into(),
            component_ids: component_ids.into(),
        }
    }

    /// A human-readable description of the failure.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The IDs of the components involved in the failure.
    pub fn component_ids(&self) -> &[u64] {
        &self.component_ids
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ValidationError {}

impl From<ValidationError> for Error {
    fn from(error: ValidationError) -> Self {
        Error::validation_errors(vec![error])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_exposes_its_message_and_components() {
        let error = ValidationError::new("boom", [3u64, 4]);
        assert_eq!(error.message(), "boom");
        assert_eq!(error.component_ids(), &[3, 4]);
        // `Display` is just the message.
        assert_eq!(error.to_string(), "boom");
    }

    #[test]
    fn leaf_error_display_is_unchanged() {
        assert_eq!(
            Error::invalid_graph("No grid component found.").to_string(),
            "InvalidGraph: No grid component found."
        );
    }

    #[test]
    fn validation_errors_display_lists_each_failure() {
        let error = Error::validation_errors(vec![
            ValidationError::new("first problem", [1u64]),
            ValidationError::new("second problem", [2u64, 3]),
        ]);
        assert_eq!(
            error.to_string(),
            "Graph validation failed:\n    first problem\n    second problem"
        );
    }

    #[test]
    fn into_validation_errors_unwraps_the_collected_failures() {
        let error: Error = ValidationError::new("boom", [1u64]).into();
        assert_eq!(
            error.into_validation_errors(),
            Ok(vec![ValidationError::new("boom", [1u64])])
        );
    }

    #[test]
    fn into_validation_errors_passes_other_kinds_through() {
        assert_eq!(
            Error::internal("bug").into_validation_errors(),
            Err(Error::internal("bug"))
        );
    }
}
