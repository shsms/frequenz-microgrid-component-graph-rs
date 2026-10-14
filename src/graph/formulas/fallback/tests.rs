// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Tests for the fallback formula engine.

use std::collections::BTreeSet;

use super::{SourcePreference, aggregate};
use crate::graph::test_utils::graph;
use crate::{ComponentGraphConfig, Error};

/// Test fallback expression generation when there are no meters in the
/// graph, with only PV inverters directly connected to the grid.
#[test]
fn test_no_meters() {
    let graph = graph! { 0: grid => { 1: inv_pv } }.build(None).unwrap();
    let expr = graph.pv_formula(None).unwrap().to_string();
    assert_eq!(expr, "COALESCE(#1, 0.0)");

    let graph = graph! { 0: grid => { 1: inv_pv, 2: inv_pv } }
        .build(None)
        .unwrap();
    let expr = graph.pv_formula(None).unwrap().to_string();
    assert_eq!(expr, "COALESCE(#1, 0.0) + COALESCE(#2, 0.0)");
}

/// Aggregation over meters and components: meter substitution for component
/// groups, the meter-vs-component source preference, meter chains, and the
/// raw-sum shortcut under `disable_fallback_components`.
#[test]
fn test_aggregate() -> Result<(), Error> {
    // A grid meter, and a battery meter with one inverter and one battery.
    let g = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
            },
        },
    };

    let graph = g.build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([1]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "#1");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "#1 + COALESCE(#3, #2, 0.0)");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

    let expr = aggregate(&graph, BTreeSet::from([3]), SourcePreference::MetersFirst)?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
    let expr = aggregate(
        &graph,
        BTreeSet::from([3]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
    let expr = aggregate(
        &graph,
        BTreeSet::from([2]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");

    let graph = g.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "#1 + #2");

    let expr = aggregate(
        &graph,
        BTreeSet::from([1, 2]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(expr.to_string(), "#1 + #2");

    let expr = aggregate(&graph, BTreeSet::from([3]), SourcePreference::MetersFirst)?;
    assert_eq!(expr.to_string(), "#3");

    let g = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
                // A battery meter with three inverters and three batteries.
                5: meter => {
                    6: inv_bat => { 9: bat, 10: bat, 11: bat },
                    7: inv_bat => { 9, 10, 11 },
                    8: inv_bat => { 9, 10, 11 },
                },
            },
        },
    };

    let graph = g.build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([3, 5]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#3, #2, 0.0) + ",
            "COALESCE(",
            "#8 + #7 + #6, ",
            "#5, ",
            "COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)",
            ")"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 5]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#2, #3, 0.0) + ",
            "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 6, 7, 8]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#2, #3, 0.0) + ",
            "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 7, 8]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#2, #3, 0.0) + ",
            "COALESCE(#5 - #6, COALESCE(#7, 0.0) + COALESCE(#8, 0.0))"
        )
    );

    let graph = g.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([3, 5]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "#3 + #5");

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 5]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(expr.to_string(), "#2 + #5");

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 6, 7, 8]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(expr.to_string(), "#2 + #6 + #7 + #8");

    let expr = aggregate(
        &graph,
        BTreeSet::from([2, 7, 8]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(expr.to_string(), "#2 + #7 + #8");

    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat } },
                5: meter => {
                    6: inv_bat => { 9: bat, 10: bat, 11: bat },
                    7: inv_bat => { 9, 10, 11 },
                    8: inv_bat => { 9, 10, 11 },
                },
                12: meter => { 13: chp, 14: inv_pv },
            },
        },
    }
    .build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([5, 12]),
        SourcePreference::MetersFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        concat!(
            "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) + ",
            "COALESCE(#12, COALESCE(#14, 0.0) + COALESCE(#13, 0.0))"
        )
    );

    let expr = aggregate(
        &graph,
        BTreeSet::from([7, 14]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(
        expr.to_string(),
        "COALESCE(#7, #5 - #6 - #8, 0.0) + COALESCE(#14, #12 - #13, 0.0)"
    );

    Ok(())
}

/// Aggregation through a meter chain: a meter measured via its single
/// (non-component) child meter when meter chains are enabled.
#[test]
fn test_aggregate_through_meter_chain() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: meter => { 4: inv_bat => { 5: bat } },
                    6: meter => { 7: inv_pv },
                },
            },
        },
    }
    .build(None)?;
    let expr = aggregate(
        &graph,
        BTreeSet::from([1]),
        SourcePreference::MetersFirstWithChains,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#1, #2)");

    Ok(())
}

/// `measurement_points` substitutes a component's predecessor meter for the
/// component, but keeps a meterless component as its own measurement point.
#[test]
fn test_measurement_points() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 2: inv_bat => { 3: bat } },
            4: inv_bat => { 5: bat },
        },
    }
    .build(None)?;

    // The metered inverter (2) resolves to its meter (1)...
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([2]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(1)],
    );
    // ...while the meterless inverter (4) is measured directly.
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([4]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(4)],
    );
    Ok(())
}

/// A component with several parallel meter parents (a diamond) is measured
/// once, through the combined meter readings, rather than double-counted by
/// summing each meter independently.
#[test]
fn test_aggregate_diamond() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 3: inv_bat => { 4: bat } },
            2: meter => { 3 },
        },
    }
    .build(None)?;
    let targets = BTreeSet::from([3]);

    // The two meters collapse into one diamond term over the inverter.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Diamond {
            components: vec![3],
            meters: vec![1, 2],
        }],
    );

    // Meters primary: their sum, then the inverter, then a best-effort sum.
    assert_eq!(
        aggregate(&graph, targets.clone(), SourcePreference::MetersFirst)?.to_string(),
        "COALESCE(#1 + #2, #3, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
    );
    // Components primary: the inverter, then the best-effort meter sum (the
    // exact meter sum is dominated by it and dropped).
    assert_eq!(
        aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
        "COALESCE(#3, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
    );
    Ok(())
}

/// A meter with only component children drills in; one with multiple
/// successors that include a meter stands alone.
#[test]
fn test_stands_alone() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: inv_bat => { 4: bat },
                3: inv_bat => { 4 },
            },
            5: meter => {
                6: meter,
                7: inv_bat => { 8: bat },
            },
        },
    }
    .build(None)?;
    assert!(!super::emit::stands_alone(
        &graph,
        1,
        SourcePreference::MetersFirst,
    )?);
    assert!(super::emit::stands_alone(
        &graph,
        5,
        SourcePreference::MetersFirstWithChains,
    )?);
    Ok(())
}

/// A stands-alone meter's fallback resolves child meters recursively, so
/// the term still evaluates when the meter and its sub-meter are offline
/// but the leaf components report.
///
/// The grid meter (Meter:1) also feeds a load (Meter:7) so Meter:2 is an
/// internal meter.
#[test]
fn test_stands_alone_total_through_child_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: inv_pv,
                    4: meter => { 5: inv_bat => { 6: bat } },
                },
                7: meter,
            },
        },
    }
    .build(None)?;
    let expr = aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?;
    // The battery sub-meter (#4) is backed by its inverter (#5), not a
    // bare `#4` that would null the sum while #5 still reports.
    assert_eq!(
        expr.to_string(),
        "COALESCE(#2, COALESCE(#4, #5, 0.0) + COALESCE(#3, 0.0))",
    );
    Ok(())
}

/// A meter substitution over an asymmetric diamond resolves to the same
/// diamond regardless of which component id is lower. The seed fed by only
/// one of the parallel meters is rejected (its sibling is also fed from
/// outside that meter's parent set), and the sibling fed by the full meter
/// set forms the diamond, subsuming the seed's standalone point.
///
/// Meter:2 feeds both inverters and Meter:3 only the shared one; built once
/// with the shared inverter's id higher and once with it lower.
#[test]
fn test_substitution_asymmetric_diamond_order_independent() -> Result<(), Error> {
    let shared_is_5 = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_pv, 5: inv_pv },
                3: meter => { 5 },
            },
        },
    };
    let shared_is_4 = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 5: inv_pv, 4: inv_pv },
                3: meter => { 4 },
            },
        },
    };
    for (shared_id_first, g) in [(false, shared_is_5), (true, shared_is_4)] {
        let graph = g.build(None)?;
        let targets = BTreeSet::from([4, 5]);
        assert_eq!(
            super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
            vec![super::resolve::Measurement::Diamond {
                components: vec![4, 5],
                meters: vec![2, 3],
            }],
            "shared_id_first: {shared_id_first}",
        );
        assert_eq!(
            aggregate(&graph, targets, SourcePreference::MetersFirst)?.to_string(),
            "COALESCE(#2 + #3, #4 + #5, COALESCE(#2, 0.0) + COALESCE(#3, 0.0))",
            "shared_id_first: {shared_id_first}",
        );
    }
    Ok(())
}

/// A meter substitution is rejected when a covered sibling is itself one
/// of the seed's parent meters. Such a sibling passes the feed check
/// against itself, but its flow already runs through the other parent's
/// reading; a diamond built from this group would count that line twice —
/// once as a parallel meter and once as a covered component. The group
/// resolves through the seed whose parent set is just the outer meter:
/// the nested feed passes `reached_only_through`, so the outer meter covers
/// the whole group as a single point.
///
/// Inverter:5's parents are {2, 4}, so Meter:4 is both a parent meter and a
/// sibling.
#[test]
fn test_substitution_rejects_parent_meter_as_sibling() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: inv_pv,
                    4: meter => { 5: inv_bat => { 6: bat } },
                    5,
                },
                7: meter,
            },
        },
    }
    .build(None)?;
    // No diamond forms over {2, 4}: with Meter:4 also in the covered
    // components it would sum #2 + #4 while all of #4's flow is already
    // inside #2. Instead the seed under only Meter:2 covers the whole
    // group — Inverter:5's nested feed through Meter:4 still counts as
    // fed through Meter:2 — and the group is one point on that meter.
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3, 4, 5]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(2)],
    );
    Ok(())
}

/// An off-limits parent meter is not substituted for the group it measures:
/// the targets resolve to their own points, as they would with no parent
/// meter at all.
#[test]
fn test_measurement_points_off_limits_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: chp },
                5: meter,
            },
        },
    }
    .build(None)?;
    let targets = BTreeSet::from([3, 4]);
    // Meter:2 measures exactly the pair, so it normally stands in for both.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(2)],
    );
    // Barred, it is not: each target becomes its own point.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::from([2]))?,
        vec![
            super::resolve::Measurement::Single(3),
            super::resolve::Measurement::Single(4)
        ],
    );
    Ok(())
}

/// A child meter that shares a component with a sibling meter (a diamond
/// one level below the meter being backed) stays a bare `#id` in the
/// children fallback sum. Recursing into both siblings would resolve each
/// to the shared component's reading and count it once per feed.
#[test]
fn test_child_meter_diamond_stays_bare() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: inv_pv,
                    4: meter => { 6: inv_pv },
                    5: meter => { 6 },
                },
                7: meter,
            },
        },
    }
    .build(None)?;
    // Meters #4 and #5 stay bare — resolving both to their shared
    // inverter (#6) would subtract its reading twice when the meters are
    // offline. The sum of bare readings is safe: each meter measures its
    // own feed line.
    assert_eq!(
        aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?.to_string(),
        "COALESCE(#2, #5 + #4 + COALESCE(#3, 0.0))",
    );
    Ok(())
}

/// A meter over a mix of target components and other meters measures the
/// targets with the meter minus the sibling meters as the meter-side
/// source, ordered against the component readings by the policy.
#[test]
fn test_aggregate_subtraction() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                // A "PV + unspecified" meter next to an unspecified sub-meter.
                2: meter => { 3: inv_pv, 4: inv_pv, 5: inv_pv, 6: inv_pv, 7: inv_pv, 8: meter },
                // A second branch off the grid meter (a building load) keeps
                // Meter:1 a grid meter and Meter:2 an internal meter, not a
                // sole-child fallback grid meter that would carry the site's
                // unmodeled load.
                9: meter,
            },
        },
    }
    .build(None)?;
    let targets = BTreeSet::from([3, 4, 5, 6, 7]);
    // The group collapses into one subtraction term over the mixed meter.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![2],
            subtracted: vec![8],
            components: vec![3, 4, 5, 6, 7],
        }],
    );
    // Meters primary: the difference, then the per-inverter sum.
    assert_eq!(
        aggregate(&graph, targets.clone(), SourcePreference::MetersFirst)?.to_string(),
        concat!(
            "COALESCE(#2 - #8, ",
            "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
            "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
        ),
    );
    // Components primary: the exact sum, the difference, then best-effort.
    assert_eq!(
        aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
        concat!(
            "COALESCE(#3 + #4 + #5 + #6 + #7, #2 - #8, ",
            "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
            "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
        ),
    );
    // The PV formula resolves to the subtraction term.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        concat!(
            "COALESCE(#3 + #4 + #5 + #6 + #7, #2 - #8, ",
            "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
            "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
        ),
    );
    // A partial group (one inverter without its mates) falls back to the
    // meter minus everything else under it — the working siblings and the
    // sub-meter.
    assert_eq!(
        graph.pv_formula(Some(BTreeSet::from([3])))?.to_string(),
        "COALESCE(#3, #2 - #4 - #5 - #6 - #7 - #8, 0.0)",
    );
    Ok(())
}

/// Subtraction generalizes to a diamond: when a target's parents are several
/// parallel meters, the group is measured as the summed meter readings minus
/// the non-target siblings.
///
/// Meter:1 is the grid meter; Meter:2 and Meter:3 are internal.
#[test]
fn test_aggregate_subtraction_diamond() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_pv, 5: inv_pv },
                3: meter => { 4, 5 },
            },
        },
    }
    .build(None)?;
    let targets = BTreeSet::from([4]);
    // One inverter behind the two parallel meters: the summed meter readings
    // minus the other inverter, with the inverter's own reading preferred.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![2, 3],
            subtracted: vec![5],
            components: vec![4],
        }],
    );
    assert_eq!(
        aggregate(&graph, targets.clone(), SourcePreference::ComponentsFirst)?.to_string(),
        "COALESCE(#4, #2 + #3 - #5, 0.0)",
    );
    assert_eq!(
        aggregate(&graph, targets, SourcePreference::MetersFirst)?.to_string(),
        "COALESCE(#2 + #3 - #5, #4, 0.0)",
    );
    // With both inverters targeted there is nothing to subtract, so it stays
    // a pure diamond.
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([4, 5]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Diamond {
            components: vec![4, 5],
            meters: vec![2, 3],
        }],
    );
    Ok(())
}

#[test]
fn test_subtraction_diamond_subsumes_earlier_single() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_pv, 5: inv_bat => { 6: bat }, 7: inv_pv },
                3: meter => { 7 },
            },
        },
    }
    .build(None)?;
    let targets = BTreeSet::from([4, 7]);
    // Asymmetric diamond: only `#7` is fed by both parallel meters, so the seed
    // `#4` resolves to a standalone `Single` first (its own subtraction sees
    // only Meter:2's parent set and is disqualified by `#7`'s feed from
    // Meter:3). The group's subtraction then covers `#4` and must subsume that
    // point, or its readings would be counted twice.
    assert_eq!(
        super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![2, 3],
            subtracted: vec![5],
            components: vec![4, 7],
        }],
    );
    assert_eq!(
        aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
        "COALESCE(#4 + #7, #2 + #3 - #5, COALESCE(#4, 0.0) + COALESCE(#7, 0.0))",
    );
    Ok(())
}

/// A diamond whose parallel parent meters are grid meters — directly under
/// the grid connection point, with mixed children so they are not component
/// meters — carries the site's unmodeled load, so the subtraction is
/// disqualified and the target falls back to its own reading, exercising the
/// per-meter grid-meter guard on the diamond path.
#[test]
fn test_subtraction_diamond_disqualified_by_grid_meters() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 3: inv_pv, 4: meter },
            2: meter => { 3, 4 },
        },
    }
    .build(None)?;
    // The parallel parents are grid meters (mixed children, under the grid),
    // so the inverter falls back to its own reading rather than the
    // meter-sum-minus-sub-meter difference.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// The subtracted siblings may be components rather than meters: a single
/// target inverter (e.g. one that is unreachable over the network) falls
/// back to the meter minus its working siblings, and a category's
/// inverters next to another category's inverter fall back to the meter
/// minus that inverter.
#[test]
fn test_aggregate_subtraction_component_siblings() -> Result<(), Error> {
    // One inverter out of three: the meter minus the others as fallback.
    let graph = graph! {
        0: grid => { 1: meter => { 2: meter => { 3: inv_pv, 4: inv_pv, 5: inv_pv } } },
    }
    .build(None)?;
    assert_eq!(
        graph.pv_formula(Some(BTreeSet::from([3])))?.to_string(),
        "COALESCE(#3, #2 - #4 - #5, 0.0)",
    );

    // A PV inverter next to a battery inverter: each category is the meter
    // minus the other category's inverter. The grid meter (Meter:1) also feeds
    // a load (Meter:6) so Meter:2 is an internal meter.
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: inv_bat => { 5: bat } },
                6: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, #2 - #4, 0.0)",
    );
    assert_eq!(
        graph.battery_formula(None)?.to_string(),
        "COALESCE(#4, #2 - #3, 0.0)",
    );
    Ok(())
}

/// The subtraction also applies when the sibling meter is a component
/// meter, in either order: PV inverters next to a battery sub-meter get
/// `mixed - battery_meter` as their meter-side source, and battery
/// inverters next to a PV sub-meter get `mixed - pv_meter`. The
/// sub-meter's own category formula is unaffected.
#[test]
fn test_aggregate_subtraction_component_sub_meter() -> Result<(), Error> {
    // The grid meter (Meter:1) also feeds a load (Meter:8) so Meter:2 is an
    // internal meter.
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: inv_pv,
                    4: inv_pv,
                    5: meter => { 6: inv_bat => { 7: bat } },
                },
                8: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3 + #4, #2 - #5, COALESCE(#3, 0.0) + COALESCE(#4, 0.0))",
    );
    assert_eq!(
        graph.battery_formula(None)?.to_string(),
        "COALESCE(#6, #5, 0.0)",
    );

    // The reverse order: battery inverters under the mixed meter, the PV system
    // behind the sub-meter. The grid meter (Meter:1) also feeds a load
    // (Meter:7) so Meter:2 is an internal meter.
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: inv_bat => { 4: bat },
                    5: meter => { 6: inv_pv },
                },
                7: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.battery_formula(None)?.to_string(),
        "COALESCE(#3, #2 - #5, 0.0)",
    );
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#6, #5, 0.0)");
    Ok(())
}

/// Shapes where the parent meter's reading can't be split cleanly fall
/// back to measuring the targets directly:
/// - a non-target sibling with no usable reading (its share is unknown);
/// - a sibling meter leading to other targets (they are measured on their
///   own, so subtracting them would drop them from the total);
/// - a parent meter directly under the grid connection point (it carries
///   the site's unmodeled consumer load);
/// - a sibling meter that is also fed from outside the parent.
#[test]
fn test_subtraction_disqualifiers() -> Result<(), Error> {
    // Non-target sibling with no usable reading (a hybrid inverter is not
    // a measurable component).
    let graph = graph! {
        0: grid => {
            1: meter => { 2: meter => { 3: inv_pv, 4: inv_hybrid => { 5: bat } } },
        },
    }
    .build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

    // Sibling meter leading to another target.
    let graph = graph! {
        0: grid => {
            1: meter => { 2: meter => { 3: inv_pv, 4: meter => { 5: inv_pv } } },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, 0.0) + COALESCE(#5, #4, 0.0)",
    );

    // Parent meter directly under the grid connection point.
    let graph = graph! { 0: grid => { 1: meter => { 2: inv_pv, 3: meter } } }.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#2, 0.0)");

    // Parent meter is a fallback grid meter: the sole child of the grid
    // meter, so it stands in for the grid connection point and likewise
    // carries the site's unmodeled consumer load.
    let graph = graph! {
        0: grid => { 1: meter => { 2: meter => { 3: inv_pv, 4: meter } } },
    }
    .build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

    // Sibling meter also fed from outside the parent.
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: meter },
                4,
            },
        },
    }
    .build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

    // Covered target also fed from outside the parent meter — here directly
    // by the grid, through no meter at all.
    // The extra inflow is not in the parent-meter reading, so the
    // meter-minus-siblings difference would undercount the group; the
    // targets are measured directly instead. Such a topology only arises
    // once neighbour validation is bypassed (the grid rule would otherwise
    // reject a second predecessor on a grid successor), so the guard is a
    // backstop for exactly that case.
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_pv, 5: inv_pv, 6: meter },
                3: meter,
            },
            5,
        },
    }
    .build(Some(
        ComponentGraphConfig::builder()
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#4, 0.0) + COALESCE(#5, 0.0)",
    );
    Ok(())
}

/// A child meter that feeds a sibling of its own contributes no term to
/// the children fallback sum: the flow it measures is already inside the
/// fed sibling's reading, so a bare `#id` next to that sibling's term
/// would count the shared line twice.
#[test]
fn test_children_fallback_drops_feeder_of_sibling() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: inv_pv,
                    4: meter => { 5: inv_bat => { 6: bat } },
                    5,
                },
                7: meter,
            },
        },
    }
    .build(None)?;
    // No bare `#4` next to `COALESCE(#5, 0.0)`: inverter #5's reading
    // already contains the flow through meter #4.
    assert_eq!(
        aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?.to_string(),
        "COALESCE(#2, COALESCE(#5, 0.0) + COALESCE(#3, 0.0))",
    );
    Ok(())
}

/// A child that feeds a sibling through a nested meter is dropped from
/// the children fallback too, not only a direct feeder: the flow through
/// the nested chain is already inside the fed sibling's reading.
#[test]
fn test_children_fallback_drops_transitive_feeder() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: meter => { 4: meter => { 5: inv_bat => { 6: bat } } },
                    5,
                },
                7: meter,
            },
        },
    }
    .build(None)?;
    // No term for meter #3 next to the inverter's reading-or-0: inverter
    // #5's reading already contains the flow through meters #3 and #4.
    assert_eq!(
        aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?.to_string(),
        "COALESCE(#2, #5, 0.0)",
    );
    Ok(())
}

/// A device child that feeds a sibling meter is dropped from the children
/// fallback: the sibling meter's reading already contains its flow. The
/// fed meter is dropped too — it is fed from outside the measured meter,
/// so its reading holds more than the measured meter passes. Such edges
/// exist only when validation failures are allowed.
#[test]
fn test_children_fallback_drops_device_feeder() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_bat => { 4: bat }, 5: meter },
                6: meter,
            },
        },
        3 => { 5 },
    }
    .build(Some(
        ComponentGraphConfig::builder()
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    // No child term at all: inverter #3 feeds sibling #5, and meter #5
    // is fed by #3 from outside meter #2's line.
    assert_eq!(
        aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?.to_string(),
        "#2",
    );
    Ok(())
}

/// A child fed by two parallel meters backs neither meter: its reading
/// holds both meters' lines, so it would overstate each. Each meter is
/// measured bare; a formula that targets the component itself measures
/// the pair as a diamond instead.
#[test]
fn test_children_fallback_drops_child_shared_with_parallel_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_bat => { 5: bat } },
                3: meter => { 4 },
                6: meter,
            },
        },
    }
    .build(None)?;
    // No `COALESCE(#_, #4, 0.0)` terms: inverter #4's reading would be
    // subtracted once per parallel meter, counting its power twice.
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([2, 3]),
            SourcePreference::MetersFirst,
        )?
        .to_string(),
        "#2 + #3",
    );
    Ok(())
}

/// A sub-meter whose `Single` point was dropped for a covering group
/// stays claimed: a later target below it must not substitute it back
/// in — its flow is already inside the covering point.
#[test]
fn test_subsumed_sub_meter_stays_claimed() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: inv_pv,
                3: meter => { 4: meter => { 6: chp }, 5: chp },
            },
        },
    }
    .build(None)?;
    // Seed order: sub-meter #4 first gets its own `Single`; CHP #5 then
    // substitutes mixed meter #3 in for the whole group and drops that
    // point. Nested CHP #6 must not claim #4 again — one term, not two.
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([4, 5, 6]),
            SourcePreference::MetersFirst,
        )?
        .to_string(),
        "COALESCE(#3, COALESCE(#5, 0.0) + COALESCE(#4, #6, 0.0))",
    );
    Ok(())
}

/// A grid meter is never backed by its children's sum, in the drilling
/// path too: its reading carries the site's unmodeled consumer load,
/// which no sum of its children accounts for. A fallback grid meter (the
/// sole child of a grid meter) still backs the outer meter as a chain.
#[test]
fn test_grid_meter_never_backed_by_children() -> Result<(), Error> {
    let graph =
        graph! { 0: grid => { 1: meter => { 2: meter => { 3: inv_pv, 4: chp } } } }.build(None)?;
    // The chain falls back from #1 to #2, but never to the device sum:
    // the devices are only the producers, so their sum would report a
    // false value while both meters are offline.
    assert_eq!(
        aggregate(
            &graph,
            BTreeSet::from([1]),
            SourcePreference::MetersFirstWithChains,
        )?
        .to_string(),
        "COALESCE(#1, #2)",
    );
    assert_eq!(
        aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?.to_string(),
        "#2",
    );
    Ok(())
}

/// A subtracted sibling fed by another subtracted sibling is not counted
/// twice. The sub-meter measures flow that is already inside the fed
/// inverter's own reading. When the sub-meter feeds only subtracted
/// siblings, it is dropped from the difference; when it also feeds
/// something else, the difference can't be split and the subtraction
/// does not apply.
#[test]
fn test_subtraction_covered_feeder() -> Result<(), Error> {
    // The sub-meter's only child is the battery inverter, which is also a
    // direct child of the mixed meter. Only the inverter is subtracted.
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: meter, 5: inv_bat => { 6: bat } },
                7: meter,
            },
        },
        4 => { 5 },
    }
    .build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, #2 - #5, 0.0)",
    );

    // The sub-meter also feeds a CHP of its own: dropping it would lose
    // the CHP's flow, keeping it would count the inverter's flow twice.
    // The subtraction does not apply.
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: meter, 5: inv_bat => { 6: bat } },
                8: meter,
            },
        },
        4 => { 5, 7: chp },
    }
    .build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// A meter fed by both the grid connection point and another meter (a
/// shape that only builds when validation failures are allowed) is a grid
/// meter no matter in which order the edges were added. Every predecessor
/// is checked, so the formula does not flip with edge order.
#[test]
fn test_grid_meter_second_feed_order_independent() -> Result<(), Error> {
    let grid_edge_added_first = graph! {
        0: grid => {
            1: meter,
            2: meter => { 3: inv_pv, 4: meter },
        },
        1 => { 2 },
    };
    let grid_edge_added_last = graph! {
        0: grid => {
            1: meter => { 2: meter => { 3: inv_pv, 4: meter } },
        },
        0 => { 2 },
    };
    for (grid_edge_first, g) in [(true, grid_edge_added_first), (false, grid_edge_added_last)] {
        let graph = g.build(Some(
            ComponentGraphConfig::builder()
                .allow_component_validation_failures(true)
                .build(),
        ))?;
        // The grid-fed meter is never a subtraction source.
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#3, 0.0)",
            "grid_edge_first: {grid_edge_first}",
        );
    }
    Ok(())
}

/// A cycle that is not reachable from the root survives validation when
/// unconnected components are allowed (the acyclicity walk starts at the
/// root). Formula generation must still stop on such a graph instead of
/// recursing forever through the cycle's predecessors.
#[test]
fn test_off_root_cycle_terminates() -> Result<(), Error> {
    let graph = graph! {
        0: grid => { 1: meter => { 2: meter => { 3: inv_pv } } },
        4: meter => { 5: meter => { 4 }, 2, 3 },
    }
    .build(Some(
        ComponentGraphConfig::builder()
            .allow_unconnected_components(true)
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    // The exact formula does not matter on an invalid graph; generating
    // it just must terminate.
    graph.pv_formula(None)?;
    Ok(())
}

/// Pins that a subtraction stays a subtraction when the redundant-feeder
/// pruning empties its subtracted set. Every subtracted sibling must then
/// have all its successors inside the subtracted set, which needs a cycle
/// among the siblings — so this input exists only on graphs kept alive by
/// the validation allow-flags. The point still carries the subtraction
/// shape (parent-meter sum, nothing subtracted), not a meter
/// substitution.
#[test]
fn test_pruned_empty_subtraction_keeps_shape() -> Result<(), Error> {
    let graph = graph! {
        0: grid => { 1: meter },
        // Off the root, with a cycle between Meter:4 and Meter:5.
        2: meter => { 3: inv_pv, 4: meter, 5: meter },
        4 => { 5 },
        5 => { 4 },
    }
    .build(Some(
        ComponentGraphConfig::builder()
            .allow_unconnected_components(true)
            .allow_component_validation_failures(true)
            .build(),
    ))?;
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Subtraction {
            parent_meters: vec![2],
            subtracted: vec![],
            components: vec![3],
        }],
    );
    Ok(())
}

/// Pins the resolver's claim bookkeeping for meter targets.
///
/// Only the consumer formula puts meters in the target set, and its
/// target collection stops at the first component chain, so a target is
/// never nested below another target meter. The scenarios here go beyond
/// that caller contract on purpose: they pin how the claim set behaves
/// today, so a restructuring that changes it is noticed. A deliberate
/// behavior change may update these expectations.
#[test]
fn test_claim_semantics_meter_targets() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: inv_pv, 5: inv_bat => { 6: bat } },
                7: meter,
            },
        },
    }
    .build(None)?;
    // The meter target claims itself as a `Single`. The inverter's
    // subtraction is then rejected — its parent meter is already claimed
    // by a point that measures more than the inverter's group — and the
    // seed falls back to a standalone point.
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([2, 3]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(2),
            super::resolve::Measurement::Single(3)
        ],
    );
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([2, 3, 4]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(2),
            super::resolve::Measurement::Single(3),
            super::resolve::Measurement::Single(4)
        ],
    );
    // With every child of the mixed meter targeted, the substitution
    // resolves the group onto the already-claimed meter: the group merges
    // into the existing point and emits nothing new.
    assert_eq!(
        super::resolve::measurement_points(
            &graph,
            &BTreeSet::from([2, 3, 4, 5]),
            &BTreeSet::new()
        )?,
        vec![super::resolve::Measurement::Single(2)],
    );
    Ok(())
}

/// Pins that a subsumed point stays claimed: a later seed must not claim
/// the same node again, because its flow is already inside the covering
/// point. The input has a target nested below a target meter, which no
/// current caller produces (see [`test_claim_semantics_meter_targets`] on
/// the caller contract); the nested target then resolves to no point of
/// its own.
#[test]
fn test_subsumed_claim_stays_claimed() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: meter => { 5: inv_bat => { 6: bat } },
                    4: inv_pv,
                },
                7: meter,
            },
        },
    }
    .build(None)?;
    // Seed 3 emits Single(3); seed 4's substitution onto Meter:2 subsumes
    // it and drops that point. Meter:3 stays claimed, so seed 5's
    // substitution onto it emits nothing — its flow is already inside
    // Single(2).
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3, 4, 5]), &BTreeSet::new())?,
        vec![super::resolve::Measurement::Single(2)],
    );
    Ok(())
}

/// A target sibling that is a meter disqualifies the subtraction: the
/// parent's reading can't be split between the meter target and the other
/// targets. Both end up as standalone points.
#[test]
fn test_subtraction_rejects_target_meter_sibling() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: meter, 4: inv_pv, 5: inv_bat => { 6: bat } },
                7: meter,
            },
        },
    }
    .build(None)?;
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([3, 4]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(3),
            super::resolve::Measurement::Single(4)
        ],
    );
    Ok(())
}

/// Two diamonds can never share a parallel meter: a shared meter means
/// one group's components are also fed by a meter that feeds the other
/// group, so the coverage check rejects both groups and each target stays
/// a standalone point. This pins that the resolver's meter claims never
/// meet an overlapping second diamond.
#[test]
fn test_no_group_across_overlapping_diamonds() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 5: inv_pv },
                3: meter => { 5, 6: inv_pv },
                4: meter => { 6 },
            },
        },
    }
    .build(None)?;
    assert_eq!(
        super::resolve::measurement_points(&graph, &BTreeSet::from([5, 6]), &BTreeSet::new())?,
        vec![
            super::resolve::Measurement::Single(5),
            super::resolve::Measurement::Single(6)
        ],
    );
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#5, 0.0) + COALESCE(#6, 0.0)",
    );
    Ok(())
}

/// The subtraction-diamond subsume resolves to the same point no matter
/// which component id is lower — the twin of
/// [`test_substitution_asymmetric_diamond_order_independent`] for the
/// subtraction path. When the shared component's id is lower, the group
/// forms directly on the first seed and there is no standalone point to
/// subsume; the result is identical.
///
/// Meter:2 feeds a PV inverter, a battery inverter and the shared PV inverter;
/// Meter:3 feeds only the shared one. Built once with the shared inverter's id
/// higher and once with it lower.
#[test]
fn test_subtraction_diamond_subsume_order_independent() -> Result<(), Error> {
    let shared_is_7 = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 4: inv_pv, 5: inv_bat => { 6: bat }, 7: inv_pv },
                3: meter => { 7 },
            },
        },
    };
    let shared_is_4 = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 7: inv_pv, 5: inv_bat => { 6: bat }, 4: inv_pv },
                3: meter => { 4 },
            },
        },
    };
    for (shared_id_first, g) in [(false, shared_is_7), (true, shared_is_4)] {
        let graph = g.build(None)?;
        let targets = BTreeSet::from([4, 7]);
        assert_eq!(
            super::resolve::measurement_points(&graph, &targets, &BTreeSet::new())?,
            vec![super::resolve::Measurement::Subtraction {
                parent_meters: vec![2, 3],
                subtracted: vec![5],
                components: vec![4, 7],
            }],
            "shared_id_first: {shared_id_first}",
        );
    }
    Ok(())
}

/// A component that provides no telemetry is dropped as a measurement source
/// but still measured through its meter, and still classifies that meter.
#[test]
fn test_no_telemetry_component_measured_via_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => { 1: meter => { 2: meter => { 3: inv_pv, 4: inv_pv [ControlOnly] } } },
    }
    .build(None)?;
    // The no-telemetry inverter #4 is not a measurement source: the meter #2
    // measures it, and only the reporting inverter #3 backs the meter.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#2, #3, 0.0)",);
    // It still classifies the meter as a PV meter.
    assert!(graph.is_pv_meter(2)?);
    assert!(graph.is_pv_chain(4)?);
    Ok(())
}

/// A reporting meter whose children all lack telemetry stands alone on its
/// reading, backed by 0.0 so the term stays total.
#[test]
fn test_no_telemetry_children_meter_stays_total() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv [ControlOnly] },
                4: meter,
            },
        },
    }
    .build(None)?;
    // Meter #2's reading is the only source for PV:3, but the term must not
    // go null with it.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#2, 0.0)");
    Ok(())
}

/// A no-telemetry component with no meter to measure it contributes nothing
/// (its reading is never emitted).
#[test]
fn test_no_telemetry_meterless_component_dropped() -> Result<(), Error> {
    let graph = graph! { 0: grid => { 1: inv_pv [Inactive] } }.build(None)?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "0.0");
    Ok(())
}

/// In a diamond, a no-telemetry target drops out of the component-side term,
/// leaving the meters as the sole source of the group total.
#[test]
fn test_no_telemetry_in_diamond() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 3: inv_pv, 4: inv_pv [ControlOnly] },
            2: meter => { 3, 4 },
        },
    }
    .build(None)?;
    // The component sum can't represent the group (only #3 reports), so the
    // meters are the total; #4 is absent.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#1 + #2, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
    );
    Ok(())
}

/// In a subtraction, a no-telemetry target drops out of the component-side
/// terms, so the meter-minus-siblings difference is the primary source.
#[test]
fn test_no_telemetry_in_subtraction() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: inv_pv [ControlOnly], 5: meter },
                // Keeps Meter:2 from being a sole-child fallback grid meter.
                6: meter,
            },
        },
    }
    .build(None)?;
    // Only #3 reports, so the exact component sum can't be primary: the
    // difference #2 - #5 is, with #3 as the degraded fallback. #4 is absent.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#2 - #5, #3, 0.0)",
    );
    Ok(())
}

/// In a subtraction where every target lacks telemetry, no component reading
/// backs the difference, so it falls back to `0.0` rather than going null
/// when the parent meter is missing.
#[test]
fn test_all_no_telemetry_in_subtraction() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => {
                    3: inv_pv [ControlOnly],
                    4: inv_pv [ControlOnly],
                    5: meter,
                },
                // Keeps Meter:2 from being a sole-child fallback grid meter.
                6: meter,
            },
        },
    }
    .build(None)?;
    // Neither #3 nor #4 reports, so the difference #2 - #5 is the sole
    // source, backed by 0.0 so the term stays total. #3 and #4 are absent.
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#2 - #5, 0.0)",
    );
    Ok(())
}

/// Under `disable_fallback_components`, a no-telemetry target is dropped
/// from the raw component sum.
#[test]
fn test_no_telemetry_disable_fallback() -> Result<(), Error> {
    let graph = graph! { 0: grid => { 1: meter => { 2: inv_pv, 3: inv_pv [ControlOnly] } } }
        .build(Some(
            ComponentGraphConfig::builder()
                .disable_fallback_components(true)
                .build(),
        ))?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "#2");
    Ok(())
}

/// Under `disable_fallback_components`, when every target is dropped for lack
/// of telemetry the term is kept total with a `0.0` (not an empty formula).
#[test]
fn test_no_telemetry_disable_fallback_all_dropped() -> Result<(), Error> {
    let graph = graph! { 0: grid => { 1: meter => { 2: inv_pv [ControlOnly] } } }.build(Some(
        ComponentGraphConfig::builder()
            .disable_fallback_components(true)
            .build(),
    ))?;
    assert_eq!(graph.pv_formula(None)?.to_string(), "0.0");
    Ok(())
}

/// A meter that provides no telemetry has no reading of its own: a component
/// under it is measured directly, and drilling the meter itself never emits
/// its `#id`.
#[test]
fn test_no_telemetry_meter_dropped() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter [Inactive] => { 3: inv_pv },
                4: meter,
            },
        },
    }
    .build(None)?;
    // The PV inverter #3 is under the no-telemetry meter #2, so it is measured
    // directly; #2 never appears.
    assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
    // Drilling the no-telemetry meter directly measures it by its children,
    // never emitting its own #2.
    let expr = aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?;
    assert_eq!(expr.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// A non-target sibling that provides no telemetry disqualifies the
/// meter-minus-siblings subtraction (its share can't be subtracted), so the
/// targets are measured directly instead.
#[test]
fn test_no_telemetry_sibling_disqualifies_subtraction() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: inv_pv, 4: inv_pv [ControlOnly], 5: meter },
                6: meter,
            },
        },
    }
    .build(None)?;
    // Targeting only PV:3: the non-target sibling PV:4 has no telemetry, so
    // the subtraction #2 - ... can't isolate the group — PV:3 is measured
    // directly, and neither #2, #4, nor #5 appear.
    let expr = aggregate(
        &graph,
        BTreeSet::from([3]),
        SourcePreference::ComponentsFirst,
    )?;
    assert_eq!(expr.to_string(), "COALESCE(#3, 0.0)");
    Ok(())
}

/// A child meter that feeds a sibling without telemetry keeps its own term:
/// the sibling has no reading to double count, and the feeder's reading is
/// the only source for that line.
#[test]
fn test_no_telemetry_sibling_keeps_feeder_meter() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => {
                2: meter => { 3: meter, 4: inv_pv [ControlOnly] },
                5: meter,
            },
        },
        3 => { 4 },
    }
    .build(None)?;
    // The feeder meter (#3) stays in the fallback sum: dropping it would
    // leave the line it measures with no source at all.
    let expr = aggregate(&graph, BTreeSet::from([2]), SourcePreference::MetersFirst)?;
    assert_eq!(expr.to_string(), "COALESCE(#2, #3)");
    Ok(())
}

/// A no-telemetry meter sibling has no reading to subtract, so its parent
/// cannot form a substitution or subtraction group: the reporting targets
/// are measured directly instead.
#[test]
fn test_no_telemetry_meter_sibling_blocks_group() -> Result<(), Error> {
    let graph = graph! {
        0: grid => {
            1: meter => { 2: meter => { 3: inv_pv, 4: meter [Inactive] => { 5: inv_pv } } },
        },
    }
    .build(None)?;
    assert_eq!(
        graph.pv_formula(None)?.to_string(),
        "COALESCE(#3, 0.0) + COALESCE(#5, 0.0)"
    );
    Ok(())
}
