// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the traits that need to be implemented by the types
//! that represent a node and an edge.

use crate::component_category::ComponentCategory;

/**
This trait needs to be implemented by the type that represents a node.

Read more about why this is necessary [here][crate#the-node-and-edge-traits].

<details>
<summary>Example implementation for microgrid API v0.17:</summary>

```ignore
impl frequenz_microgrid_component_graph::Node for common::v1::microgrid::components::Component {
    fn component_id(&self) -> u64 {
        self.id as u64
    }

    fn category(&self) -> frequenz_microgrid_component_graph::ComponentCategory {
        use common::v1::microgrid::components as pb;
        use frequenz_microgrid_component_graph as gr;

        let category = pb::ComponentCategory::try_from(self.category).unwrap_or_else(|e| {
            error!("Error converting component category: {}", e);
            pb::ComponentCategory::Unspecified
        });

        match category {
            pb::ComponentCategory::Unspecified => gr::ComponentCategory::Unspecified,
            pb::ComponentCategory::Grid => gr::ComponentCategory::Grid,
            pb::ComponentCategory::Meter => gr::ComponentCategory::Meter,
            pb::ComponentCategory::Inverter => {
                gr::ComponentCategory::Inverter(match self.category_type {
                    Some(pb::ComponentCategoryMetadataVariant { metadata }) => match metadata {
                        Some(pb::component_category_metadata_variant::Metadata::Inverter(
                            inverter,
                        )) => {
                            match pb::InverterType::try_from(inverter.r#type).unwrap_or_else(|e| {
                                error!("Error converting inverter type: {}", e);
                                pb::InverterType::Unspecified
                            }) {
                                pb::InverterType::Solar => gr::InverterType::Solar,
                                pb::InverterType::Battery => gr::InverterType::Battery,
                                pb::InverterType::Hybrid => gr::InverterType::Hybrid,
                                pb::InverterType::Unspecified => gr::InverterType::Unspecified,
                            }
                        }
                        Some(_) => {
                            warn!("Unknown metadata variant for inverter: {:?}", metadata);
                            gr::InverterType::Unspecified
                        }
                        None => gr::InverterType::Unspecified,
                    },
                    _ => gr::InverterType::Unspecified,
                })
            }
            pb::ComponentCategory::Converter => gr::ComponentCategory::Converter,
            pb::ComponentCategory::Battery => {
                gr::ComponentCategory::Battery(match self.category_type {
                    Some(pb::ComponentCategoryMetadataVariant { metadata }) => match metadata {
                        Some(pb::component_category_metadata_variant::Metadata::Battery(
                            battery,
                        )) => match pb::BatteryType::try_from(battery.r#type).unwrap_or_else(|e| {
                            error!("Error converting battery type: {}", e);
                            pb::BatteryType::Unspecified
                        }) {
                            pb::BatteryType::LiIon => gr::BatteryType::LiIon,
                            pb::BatteryType::NaIon => gr::BatteryType::NaIon,
                            pb::BatteryType::Unspecified => gr::BatteryType::Unspecified,
                        },
                        Some(_) => {
                            warn!("Unknown metadata variant for battery: {:?}", metadata);
                            gr::BatteryType::Unspecified
                        }
                        None => gr::BatteryType::Unspecified,
                    },
                    _ => gr::BatteryType::Unspecified,
                })
            }
            pb::ComponentCategory::EvCharger => {
                gr::ComponentCategory::EvCharger(match self.category_type {
                    Some(pb::ComponentCategoryMetadataVariant { metadata }) => match metadata {
                        Some(pb::component_category_metadata_variant::Metadata::EvCharger(
                            ev_charger,
                        )) => match pb::EvChargerType::try_from(ev_charger.r#type).unwrap_or_else(
                            |e| {
                                error!("Error converting ev charger type: {}", e);
                                pb::EvChargerType::Unspecified
                            },
                        ) {
                            pb::EvChargerType::Ac => gr::EvChargerType::Ac,
                            pb::EvChargerType::Dc => gr::EvChargerType::Dc,
                            pb::EvChargerType::Hybrid => gr::EvChargerType::Hybrid,
                            pb::EvChargerType::Unspecified => gr::EvChargerType::Unspecified,
                        },
                        Some(_) => {
                            warn!("Unknown metadata variant for ev charger: {:?}", metadata);
                            gr::EvChargerType::Unspecified
                        }
                        None => gr::EvChargerType::Unspecified,
                    },
                    _ => gr::EvChargerType::Unspecified,
                })
            }
            pb::ComponentCategory::CryptoMiner => gr::ComponentCategory::CryptoMiner,
            pb::ComponentCategory::Electrolyzer => gr::ComponentCategory::Electrolyzer,
            pb::ComponentCategory::Chp => gr::ComponentCategory::Chp,
            pb::ComponentCategory::Relay => gr::ComponentCategory::Relay,
            pb::ComponentCategory::Precharger => gr::ComponentCategory::Precharger,
            pb::ComponentCategory::Fuse => gr::ComponentCategory::Fuse,
            pb::ComponentCategory::VoltageTransformer => gr::ComponentCategory::VoltageTransformer,
            pb::ComponentCategory::Hvac => gr::ComponentCategory::Hvac,
        }
    }

    fn is_supported(&self) -> bool {
        self.status != common::v1::microgrid::components::ComponentStatus::Inactive as i32
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
    /// Returns true if the component can be read from and/or controlled.
    fn is_supported(&self) -> bool;
}

/**
This trait needs to be implemented by the type that represents a connection.

Read more about why this is necessary [here][crate#the-node-and-edge-traits].

<details>
<summary>Example implementation for microgrid API v0.17:</summary>

```ignore
impl frequenz_microgrid_component_graph::Edge
    for common::v1::microgrid::components::ComponentConnection
{
    fn source(&self) -> u64 {
        self.source_component_id
    }

    fn destination(&self) -> u64 {
        self.destination_component_id
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
