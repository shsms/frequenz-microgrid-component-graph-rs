// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the traits that need to be implemented by the types
//! that represent a node and an edge.

use crate::component_category::ComponentCategory;
use crate::operational_mode::OperationalMode;

/**
This trait needs to be implemented by the type that represents a node.

Read more about why this is necessary [here][crate#the-node-and-edge-traits].

<details>
<summary>Example implementation for microgrid API v0.18.1:</summary>

```ignore
impl frequenz_microgrid_component_graph::Node
    for common::v1alpha8::microgrid::electrical_components::ElectricalComponent
{
    fn component_id(&self) -> u64 {
        self.id
    }

    fn category(&self) -> frequenz_microgrid_component_graph::ComponentCategory {
        use common::v1alpha8::microgrid::electrical_components as pb;
        use frequenz_microgrid_component_graph as gr;

        let category = pb::ElectricalComponentCategory::try_from(self.category)
            .unwrap_or_else(|e| {
                error!("Error converting component category: {}", e);
                pb::ElectricalComponentCategory::Unspecified
            });

        let specific_info = self.category_specific_info.as_ref().and_then(|info| info.kind);

        match category {
            pb::ElectricalComponentCategory::Unspecified => gr::ComponentCategory::Unspecified,
            pb::ElectricalComponentCategory::GridConnectionPoint => {
                gr::ComponentCategory::GridConnectionPoint
            }
            pb::ElectricalComponentCategory::Meter => gr::ComponentCategory::Meter,
            pb::ElectricalComponentCategory::Inverter => {
                use pb::electrical_component_category_specific_info::Kind;
                gr::ComponentCategory::Inverter(match specific_info {
                    Some(Kind::Inverter(inverter)) => {
                        match pb::InverterType::try_from(inverter.r#type).unwrap_or_else(|e| {
                            error!("Error converting inverter type: {}", e);
                            pb::InverterType::Unspecified
                        }) {
                            pb::InverterType::Pv => gr::InverterType::Pv,
                            pb::InverterType::Battery => gr::InverterType::Battery,
                            pb::InverterType::Hybrid => gr::InverterType::Hybrid,
                            pb::InverterType::Unspecified => gr::InverterType::Unspecified,
                        }
                    }
                    Some(other) => {
                        warn!("Unknown category-specific info for inverter: {:?}", other);
                        gr::InverterType::Unspecified
                    }
                    None => gr::InverterType::Unspecified,
                })
            }
            pb::ElectricalComponentCategory::Converter => gr::ComponentCategory::Converter,
            pb::ElectricalComponentCategory::Battery => {
                use pb::electrical_component_category_specific_info::Kind;
                gr::ComponentCategory::Battery(match specific_info {
                    Some(Kind::Battery(battery)) => {
                        match pb::BatteryType::try_from(battery.r#type).unwrap_or_else(|e| {
                            error!("Error converting battery type: {}", e);
                            pb::BatteryType::Unspecified
                        }) {
                            pb::BatteryType::LiIon => gr::BatteryType::LiIon,
                            pb::BatteryType::NaIon => gr::BatteryType::NaIon,
                            pb::BatteryType::Unspecified => gr::BatteryType::Unspecified,
                        }
                    }
                    Some(other) => {
                        warn!("Unknown category-specific info for battery: {:?}", other);
                        gr::BatteryType::Unspecified
                    }
                    None => gr::BatteryType::Unspecified,
                })
            }
            pb::ElectricalComponentCategory::EvCharger => {
                use pb::electrical_component_category_specific_info::Kind;
                gr::ComponentCategory::EvCharger(match specific_info {
                    Some(Kind::EvCharger(ev_charger)) => {
                        match pb::EvChargerType::try_from(ev_charger.r#type).unwrap_or_else(|e| {
                            error!("Error converting ev charger type: {}", e);
                            pb::EvChargerType::Unspecified
                        }) {
                            pb::EvChargerType::Ac => gr::EvChargerType::Ac,
                            pb::EvChargerType::Dc => gr::EvChargerType::Dc,
                            pb::EvChargerType::Hybrid => gr::EvChargerType::Hybrid,
                            pb::EvChargerType::Unspecified => gr::EvChargerType::Unspecified,
                        }
                    }
                    Some(other) => {
                        warn!("Unknown category-specific info for ev charger: {:?}", other);
                        gr::EvChargerType::Unspecified
                    }
                    None => gr::EvChargerType::Unspecified,
                })
            }
            pb::ElectricalComponentCategory::Breaker => gr::ComponentCategory::Breaker,
            pb::ElectricalComponentCategory::Precharger => gr::ComponentCategory::Precharger,
            pb::ElectricalComponentCategory::Chp => gr::ComponentCategory::Chp,
            pb::ElectricalComponentCategory::Electrolyzer => gr::ComponentCategory::Electrolyzer,
            pb::ElectricalComponentCategory::PowerTransformer => {
                gr::ComponentCategory::PowerTransformer
            }
            pb::ElectricalComponentCategory::Hvac => gr::ComponentCategory::Hvac,
            pb::ElectricalComponentCategory::Plc => gr::ComponentCategory::Plc,
            pb::ElectricalComponentCategory::CryptoMiner => gr::ComponentCategory::CryptoMiner,
            pb::ElectricalComponentCategory::StaticTransferSwitch => {
                gr::ComponentCategory::StaticTransferSwitch
            }
            pb::ElectricalComponentCategory::UninterruptiblePowerSupply => {
                gr::ComponentCategory::UninterruptiblePowerSupply
            }
            pb::ElectricalComponentCategory::CapacitorBank => {
                gr::ComponentCategory::CapacitorBank
            }
            pb::ElectricalComponentCategory::WindTurbine => gr::ComponentCategory::WindTurbine,
            pb::ElectricalComponentCategory::SteamBoiler => gr::ComponentCategory::SteamBoiler,
        }
    }

    fn operational_mode(&self) -> frequenz_microgrid_component_graph::OperationalMode {
        use common::v1alpha8::microgrid::electrical_components as pb;
        use frequenz_microgrid_component_graph as gr;

        let mode = pb::ElectricalComponentOperationalMode::try_from(self.operational_mode)
            .unwrap_or_else(|e| {
                error!("Error converting operational mode: {}", e);
                pb::ElectricalComponentOperationalMode::Unspecified
            });

        match mode {
            pb::ElectricalComponentOperationalMode::Unspecified => {
                gr::OperationalMode::Unspecified
            }
            pb::ElectricalComponentOperationalMode::Inactive => gr::OperationalMode::Inactive,
            pb::ElectricalComponentOperationalMode::TelemetryOnly => {
                gr::OperationalMode::TelemetryOnly
            }
            pb::ElectricalComponentOperationalMode::ControlOnly => {
                gr::OperationalMode::ControlOnly
            }
            pb::ElectricalComponentOperationalMode::ControlAndTelemetry => {
                gr::OperationalMode::ControlAndTelemetry
            }
        }
    }
}
```

</details>
*/
pub trait Node {
    /// Returns the component id of the component.
    fn component_id(&self) -> u64;
    /// Returns the category of the category.
    fn category(&self) -> ComponentCategory;
    /// Returns the operational mode of the component.
    ///
    /// The default implementation returns [`OperationalMode::Unspecified`],
    /// which is treated as providing telemetry. An implementor that does not
    /// override this method keeps every component usable as a measurement
    /// source in formulas.
    ///
    /// A component whose mode does not provide telemetry (see
    /// [`OperationalMode::provides_telemetry`]) is not used as a measurement
    /// source in formulas. It is still used to classify the meter that
    /// measures it (e.g. as a PV meter or a CHP meter).
    fn operational_mode(&self) -> OperationalMode {
        OperationalMode::Unspecified
    }
}

/**
This trait needs to be implemented by the type that represents a connection.

Read more about why this is necessary [here][crate#the-node-and-edge-traits].

<details>
<summary>Example implementation for microgrid API v0.18.1:</summary>

```ignore
impl frequenz_microgrid_component_graph::Edge
    for common::v1alpha8::microgrid::electrical_components::ElectricalComponentConnection
{
    fn source(&self) -> u64 {
        self.source_electrical_component_id
    }

    fn destination(&self) -> u64 {
        self.destination_electrical_component_id
    }
}
```

</details>
*/
pub trait Edge {
    /// Returns the source component id of the connection.
    fn source(&self) -> u64;
    /// Returns the destination component id of the connection.
    fn destination(&self) -> u64;
}
