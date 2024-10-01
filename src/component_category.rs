// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module defines the `ComponentCategory` enum, which represents the
//! category of a component.

use crate::graph_traits::Node;
use std::fmt::Display;

/// Represents the type of an inverter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InverterType {
    Unspecified,
    Solar,
    Battery,
    Hybrid,
}

impl Display for InverterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InverterType::Unspecified => write!(f, "Unspecified"),
            InverterType::Solar => write!(f, "Solar"),
            InverterType::Battery => write!(f, "Battery"),
            InverterType::Hybrid => write!(f, "Hybrid"),
        }
    }
}

/// Represents the category of a component.
///
/// Values of the underlying generated `ComponentCategory` and `ComponentType` types
/// need to be converted to this type, so that they can be used in the
/// `ComponentGraph`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ComponentCategory {
    Unspecified,
    Grid,
    Meter,
    Battery,
    Inverter(InverterType),
    EvCharger,
    Converter,
    CryptoMiner,
    Electrolyzer,
    Chp,
    Precharger,
    Fuse,
    VoltageTransformer,
    Hvac,
    Relay,
}

impl Display for ComponentCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentCategory::Unspecified => write!(f, "Unspecified"),
            ComponentCategory::Grid => write!(f, "Grid"),
            ComponentCategory::Meter => write!(f, "Meter"),
            ComponentCategory::Battery => write!(f, "Battery"),
            ComponentCategory::Inverter(inverter_type) => write!(f, "{}Inverter", inverter_type),
            ComponentCategory::EvCharger => write!(f, "EVCharger"),
            ComponentCategory::Converter => write!(f, "Converter"),
            ComponentCategory::CryptoMiner => write!(f, "CryptoMiner"),
            ComponentCategory::Electrolyzer => write!(f, "Electrolyzer"),
            ComponentCategory::Chp => write!(f, "CHP"),
            ComponentCategory::Precharger => write!(f, "Precharger"),
            ComponentCategory::Fuse => write!(f, "Fuse"),
            ComponentCategory::VoltageTransformer => write!(f, "VoltageTransformer"),
            ComponentCategory::Hvac => write!(f, "HVAC"),
            ComponentCategory::Relay => write!(f, "Relay"),
        }
    }
}

/// Predicates for checking the component category of a `Node`.
pub(crate) trait CategoryPredicates: Node {
    fn is_unspecified(&self) -> bool {
        self.category() == ComponentCategory::Unspecified
    }

    fn is_grid(&self) -> bool {
        self.category() == ComponentCategory::Grid
    }

    fn is_meter(&self) -> bool {
        self.category() == ComponentCategory::Meter
    }

    fn is_battery(&self) -> bool {
        self.category() == ComponentCategory::Battery
    }

    fn is_inverter(&self) -> bool {
        matches!(self.category(), ComponentCategory::Inverter(_))
    }

    fn is_battery_inverter(&self) -> bool {
        self.category() == ComponentCategory::Inverter(InverterType::Battery)
    }

    fn is_pv_inverter(&self) -> bool {
        self.category() == ComponentCategory::Inverter(InverterType::Solar)
    }

    fn is_unspecified_inverter(&self) -> bool {
        self.category() == ComponentCategory::Inverter(InverterType::Unspecified)
    }

    fn is_ev_charger(&self) -> bool {
        self.category() == ComponentCategory::EvCharger
    }

    fn is_chp(&self) -> bool {
        self.category() == ComponentCategory::Chp
    }
}

/// Implement the `CategoryPredicates` trait for all types that implement the
/// `Node` trait.
impl<T: Node> CategoryPredicates for T {}
