// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the configuration options for the `ComponentGraph`.

/// Configuration options for the `ComponentGraph`.
#[derive(Clone, Default, Debug)]
pub struct ComponentGraphConfig {
    /// Whether to allow untyped inverters in the graph.  When this is `true`,
    /// inverters that have `InverterType::Unspecified` will be assumed to be
    /// Battery inverters.
    pub allow_unspecified_inverters: bool,
}
