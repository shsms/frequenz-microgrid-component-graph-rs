// License: MIT
// Copyright © 2026 Frequenz Energy-as-a-Service GmbH

//! This module defines the [`OperationalMode`] enum, which describes whether a
//! component is active and whether it provides telemetry, accepts control
//! commands, or both.

use std::fmt::Display;

/// The operational mode of a component.
///
/// Mirrors the `ElectricalComponentOperationalMode` enum in the microgrid API.
/// It indicates whether a component is active and operational, and whether it
/// provides telemetry data, accepts control commands, or both.
///
/// A component that does not provide telemetry (see [`Self::provides_telemetry`])
/// cannot be a measurement source in a formula, but it can still be used to
/// classify the meter that measures it (e.g. as a PV meter or a CHP meter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OperationalMode {
    /// The operational mode is not explicitly set.
    ///
    /// Treated as if the component provides telemetry. A component with an
    /// unknown mode stays usable as a measurement source in formulas.
    #[default]
    Unspecified,
    /// The component is inactive and not operational. It does not provide
    /// telemetry data, and it does not accept control commands.
    Inactive,
    /// The component is active and operational. It only provides telemetry data,
    /// and it does not accept control commands.
    TelemetryOnly,
    /// The component is active and operational. It only accepts control commands,
    /// and it does not provide telemetry data.
    ControlOnly,
    /// The component is active and operational. It provides telemetry data and
    /// accepts control commands.
    ControlAndTelemetry,
}

impl OperationalMode {
    /// Returns `true` if a component in this mode provides telemetry data.
    ///
    /// [`Self::Unspecified`] is treated as providing telemetry: when the mode is
    /// unknown the component keeps the default behavior of being usable as a
    /// measurement source.
    pub fn provides_telemetry(self) -> bool {
        match self {
            OperationalMode::Unspecified
            | OperationalMode::TelemetryOnly
            | OperationalMode::ControlAndTelemetry => true,
            OperationalMode::Inactive | OperationalMode::ControlOnly => false,
        }
    }

    /// How the mode reads in explanation prose, as a predicate phrase:
    /// "meter #4 is inactive", "inverter #7 is in control-only mode".
    pub(crate) fn describe(self) -> &'static str {
        match self {
            OperationalMode::Unspecified => "has an unspecified operational mode",
            OperationalMode::Inactive => "is inactive",
            OperationalMode::TelemetryOnly => "is in telemetry-only mode",
            OperationalMode::ControlOnly => "is in control-only mode",
            OperationalMode::ControlAndTelemetry => "is in control-and-telemetry mode",
        }
    }
}

impl Display for OperationalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationalMode::Unspecified => write!(f, "Unspecified"),
            OperationalMode::Inactive => write!(f, "Inactive"),
            OperationalMode::TelemetryOnly => write!(f, "TelemetryOnly"),
            OperationalMode::ControlOnly => write!(f, "ControlOnly"),
            OperationalMode::ControlAndTelemetry => write!(f, "ControlAndTelemetry"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provides_telemetry() {
        // Modes that provide telemetry (Unspecified assumed to, for backward
        // compatibility).
        for mode in [
            OperationalMode::Unspecified,
            OperationalMode::TelemetryOnly,
            OperationalMode::ControlAndTelemetry,
        ] {
            assert!(mode.provides_telemetry(), "{mode} should provide telemetry");
        }

        // Modes that do not provide telemetry.
        for mode in [OperationalMode::Inactive, OperationalMode::ControlOnly] {
            assert!(
                !mode.provides_telemetry(),
                "{mode} should not provide telemetry"
            );
        }
    }

    #[test]
    fn test_default_is_unspecified() {
        assert_eq!(OperationalMode::default(), OperationalMode::Unspecified);
    }
}
