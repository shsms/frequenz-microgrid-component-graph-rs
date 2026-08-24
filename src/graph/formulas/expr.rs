// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

use crate::Node;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    /// An empty expression, which as a formula would evaluate to None.
    None,

    /// A negation of an expression.
    Neg { param: Box<Expr> },

    /// A numeric constant.
    Number { value: f64 },

    /// A reference to a component.
    Component { component_id: u64 },

    /// An addition of multiple expressions.
    Add { params: Vec<Expr> },

    /// A subtraction of multiple expressions.
    Sub { params: Vec<Expr> },

    /// A coalesce function.
    Coalesce { params: Vec<Expr> },

    /// A min function.
    Min { params: Vec<Expr> },

    /// A max function.
    Max { params: Vec<Expr> },
}

impl std::ops::Add for Expr {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::None, other) | (other, Self::None) => other,
            // -a + -b = -(a + b)
            (Self::Neg { param: lhs }, Self::Neg { param: rhs }) => -(*lhs + *rhs),
            // -a + b = b - a
            // a + -b = a - b
            (other, Self::Neg { param }) | (Self::Neg { param }, other) => other - *param,
            // (a + b) + (c + d) = a + b + c + d
            (Self::Add { params: mut lhs }, Self::Add { params: mut rhs }) => {
                lhs.append(&mut rhs);
                Self::Add { params: lhs }
            }
            // (a + b) + c = a + b + c
            (Self::Add { mut params }, rhs) => {
                params.push(rhs);
                Self::Add { params }
            }
            // a + (b + c) = a + b + c
            (lhs, Self::Add { mut params }) => {
                params.insert(0, lhs);
                Self::Add { params }
            }
            // Catch all other cases
            (lhs, rhs) => Self::Add {
                params: vec![lhs, rhs],
            },
        }
    }
}

impl std::ops::Sub for Expr {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::None, other) => -other,
            (other, Self::None) => other,
            // (a - b) - -c = a - b + c
            (sub @ Self::Sub { .. }, Self::Neg { param }) => sub + *param,
            // -a - (b - c) = c - b - a
            (Self::Neg { param }, sub @ Self::Sub { .. }) => -sub - *param,
            // (a - b) - c = a - b - c
            (Self::Sub { mut params }, rhs) => {
                params.push(rhs);
                Self::Sub { params }
            }
            // -a - -b = b - a
            (Self::Neg { param: lhs }, Self::Neg { param: rhs }) => Self::Sub {
                params: vec![*rhs, *lhs],
            },
            // -a - b = -(a + b)
            (Self::Neg { param }, value) => -(*param + value),
            // a - -b = a + b
            (lhs, Self::Neg { param }) => lhs + *param,
            // Catch all other cases
            (lhs, rhs) => Self::Sub {
                params: vec![lhs, rhs],
            },
        }
    }
}

impl std::ops::Neg for Expr {
    type Output = Self;

    fn neg(self) -> Self {
        match self {
            Self::None => Self::None,
            // -(-a) = a
            Expr::Neg { param: inner } => *inner,
            // -(a - b) = b - a
            // -(a - b - c) = b + c - a
            // (`Sub` always has at least two operands by construction; the guard
            // keeps `remove(0)` from panicking should that ever change.)
            Expr::Sub { mut params } if !params.is_empty() => {
                let first = params.remove(0);
                Expr::Add { params } - first
            }
            // Catch all other cases
            _ => Expr::Neg {
                param: Box::new(self),
            },
        }
    }
}

impl<N: Node> From<&N> for Expr {
    fn from(node: &N) -> Self {
        Self::Component {
            component_id: node.component_id(),
        }
    }
}

/// Constructors for `FormulaExpression`.
impl Expr {
    #[must_use]
    pub(crate) fn number(value: f64) -> Self {
        Self::Number { value }
    }

    #[must_use]
    pub(crate) fn component(component_id: u64) -> Self {
        Self::Component { component_id }
    }

    #[must_use]
    pub(crate) fn coalesce(self, other: Expr) -> Self {
        match (self, other) {
            (Expr::None, other) | (other, Expr::None) => other,
            (
                Expr::Coalesce { mut params },
                Expr::Coalesce {
                    params: other_params,
                },
            ) => {
                // If both parameters are coalesce expressions, merge them.
                params.extend(other_params);
                Self::Coalesce { params }
            }
            (Expr::Coalesce { mut params }, other) => {
                // If the first parameter is a coalesce expression, add the second
                // parameter to it.
                params.push(other);
                Self::Coalesce { params }
            }
            (
                param,
                Expr::Coalesce {
                    params: other_params,
                },
            ) => {
                // If the second parameter is a coalesce expression, add the first
                // parameter to it.
                let mut params = vec![param];
                params.extend(other_params);
                Self::Coalesce { params }
            }
            (first, second) => {
                // If neither parameter is a coalesce expression, create a new one.
                Self::Coalesce {
                    params: vec![first, second],
                }
            }
        }
    }

    #[must_use]
    pub(crate) fn min(self, other: Expr) -> Self {
        match (self, other) {
            (Expr::None, expr) | (expr, Expr::None) => expr,
            (
                Expr::Min { mut params },
                Expr::Min {
                    params: other_params,
                },
            ) => {
                // If both parameters are min expressions, merge them.
                params.extend(other_params);
                Self::Min { params }
            }
            (Expr::Min { mut params }, other) | (other, Expr::Min { mut params }) => {
                // If one parameter is a min expression, add the other parameter
                // to it.
                params.push(other);
                Self::Min { params }
            }
            (first, second) => {
                // If neither parameter is a min expression, create a new one.
                Self::Min {
                    params: vec![first, second],
                }
            }
        }
    }

    #[must_use]
    pub(crate) fn max(self, other: Expr) -> Self {
        match (self, other) {
            (Expr::None, expr) | (expr, Expr::None) => expr,
            (
                Expr::Max { mut params },
                Expr::Max {
                    params: other_params,
                },
            ) => {
                // If both parameters are max expressions, merge them.
                params.extend(other_params);
                Self::Max { params }
            }
            (Expr::Max { mut params }, other) | (other, Expr::Max { mut params }) => {
                // If one parameter is a max expression, add the other parameter
                // to it.
                params.push(other);
                Self::Max { params }
            }
            (first, second) => {
                // If neither parameter is a max expression, create a new one.
                Self::Max {
                    params: vec![first, second],
                }
            }
        }
    }
}

/// Inspection helpers for `Expr`.
impl Expr {
    /// Collects the ids of all components referenced in the expression, in
    /// order of appearance (duplicates included).
    pub(crate) fn component_ids(&self) -> Vec<u64> {
        let mut ids = Vec::new();
        self.collect_component_ids(&mut ids);
        ids
    }

    /// Whether the expression resolves to a value whatever the component
    /// readings are: a constant, or a chain that ends in one. A component
    /// reference can be missing, and the arithmetic operators propagate a
    /// missing operand, so only a `COALESCE` with a resolving parameter
    /// recovers from one.
    ///
    /// Rationales that promise a total term ("a 0.0 keeps the sum total")
    /// are true only of an expression this holds for.
    pub(crate) fn always_resolves(&self) -> bool {
        match self {
            Self::None | Self::Component { .. } => false,
            Self::Number { .. } => true,
            Self::Neg { param } => param.always_resolves(),
            Self::Coalesce { params } => params.iter().any(Self::always_resolves),
            Self::Add { params }
            | Self::Sub { params }
            | Self::Min { params }
            | Self::Max { params } => params.iter().all(Self::always_resolves),
        }
    }

    /// Whether the expression needs brackets where the grammar is ambiguous:
    /// as the operand of a negation, or after a binary `-`. Only an additive
    /// expression of more than one operand qualifies; a single-term one
    /// renders like its sole term. The single source of the grammar's
    /// bracketing rule — `Display` and the commented renderer both use it.
    pub(crate) fn needs_brackets(&self) -> bool {
        matches!(self, Self::Add { params } | Self::Sub { params } if params.len() > 1)
    }

    fn collect_component_ids(&self, ids: &mut Vec<u64>) {
        match self {
            Self::None | Self::Number { .. } => {}
            Self::Component { component_id } => ids.push(*component_id),
            Self::Neg { param } => param.collect_component_ids(ids),
            Self::Add { params }
            | Self::Sub { params }
            | Self::Coalesce { params }
            | Self::Min { params }
            | Self::Max { params } => {
                for param in params {
                    param.collect_component_ids(ids);
                }
            }
        }
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render())
    }
}

/// Display helpers for `Expr`.
///
/// Bracketing is precedence-based: only an *additive* expression (`Add` / `Sub`)
/// can be ambiguous in context, and only in two positions — as the operand of a
/// negation (`-(a + b)`) or as a non-first operand of a subtraction
/// (`a - (b + c)`). Everywhere else (additions, the first term of a
/// subtraction, and the comma-separated arguments of `COALESCE` / `MIN` /
/// `MAX`) an additive child renders without brackets, because `a + b - c`
/// already parses as `a + (b - c)`.
impl Expr {
    /// Renders the expression as a formula string.
    fn render(&self) -> String {
        match self {
            Self::None => String::from("None"),
            Self::Neg { param } => format!("-{}", param.render_grouped()),
            Self::Number { value } => {
                if value.fract() == 0.0 {
                    // For whole numbers, format with one decimal place.
                    format!("{value:.1}")
                } else {
                    // else format normally.
                    format!("{value}")
                }
            }
            Self::Component { component_id } => format!("#{component_id}"),
            Self::Add { params } => Self::join(params, " + "),
            Self::Sub { params } => match params.split_first() {
                Some((first, rest)) => {
                    let mut result = first.render();
                    for param in rest {
                        result.push_str(" - ");
                        result.push_str(&param.render_grouped());
                    }
                    result
                }
                None => String::new(),
            },
            Self::Coalesce { params } => format!("COALESCE({})", Self::join(params, ", ")),
            Self::Min { params } => format!("MIN({})", Self::join(params, ", ")),
            Self::Max { params } => format!("MAX({})", Self::join(params, ", ")),
        }
    }

    /// Renders the expression, wrapping it in brackets when it is additive (so
    /// it can be safely placed after a `-`).
    fn render_grouped(&self) -> String {
        match self.needs_brackets() {
            true => format!("({})", self.render()),
            false => self.render(),
        }
    }

    /// Renders and joins the given expressions with `separator`.
    fn join(params: &[Expr], separator: &str) -> String {
        params
            .iter()
            .map(Self::render)
            .collect::<Vec<_>>()
            .join(separator)
    }
}

#[cfg(test)]
mod tests {
    use super::Expr;

    #[track_caller]
    fn assert_expr(exprs: &[Expr], expected: &str) {
        for expr in exprs {
            assert_eq!(expr.to_string(), expected);
        }
    }

    /// A term resolves whatever the readings are only when a constant backs
    /// it: `COALESCE` recovers a missing operand, the arithmetic operators
    /// propagate it.
    #[test]
    fn test_always_resolves() {
        let comp = Expr::component;
        let number = Expr::number;
        let coalesce = |a: Expr, b: Expr| a.coalesce(b);

        for expr in [
            number(0.0),
            coalesce(comp(1), number(0.0)),
            coalesce(comp(1), number(0.0)) + coalesce(comp(2), number(0.0)),
            -coalesce(comp(1), number(0.0)),
            coalesce(comp(1), comp(2)).coalesce(number(0.0)),
            number(0.0).min(number(1.0)),
        ] {
            assert!(expr.always_resolves(), "expected total: {expr}");
        }

        for expr in [
            Expr::None,
            comp(1),
            coalesce(comp(1), comp(2)),
            // One bare reading is enough to make the whole sum missable.
            comp(1) + coalesce(comp(2), number(0.0)),
            comp(1) - number(0.0),
            -comp(1),
            number(0.0).min(comp(1)),
        ] {
            assert!(!expr.always_resolves(), "expected missable: {expr}");
        }
    }

    #[test]
    fn test_arithmatic() {
        let comp = Expr::component;

        assert_expr(
            &[
                comp(10) + comp(11) + comp(12) + comp(13),
                comp(10) - -comp(11) + (comp(12) + comp(13)),
                (comp(10) + comp(11)) - -(comp(12) - -comp(13)),
            ],
            "#10 + #11 + #12 + #13",
        );

        assert_expr(
            &[
                -(comp(10) + comp(11) + comp(12)),
                -comp(10) - comp(11) - comp(12),
                -comp(10) - (comp(11) + comp(12)),
                -(comp(10) + comp(11)) - comp(12),
            ],
            "-(#10 + #11 + #12)",
        );

        assert_expr(
            &[
                comp(11) - comp(10),
                comp(11) + -comp(10),
                -comp(10) + comp(11),
                -comp(10) - -comp(11),
            ],
            "#11 - #10",
        );

        assert_expr(
            &[
                (comp(11) + comp(12)) - comp(10),
                (comp(11) + comp(12)) + -comp(10),
                -comp(10) + (comp(11) + comp(12)),
                -comp(10) - -(comp(11) + comp(12)),
            ],
            "#11 + #12 - #10",
        );

        assert_expr(
            &[
                (comp(11) - comp(12)) - comp(10),
                (comp(11) - comp(12)) + -comp(10),
                -comp(10) + (comp(11) - comp(12)),
                -comp(10) - -(comp(11) - comp(12)),
            ],
            "#11 - #12 - #10",
        );

        assert_expr(
            &[
                comp(11) - comp(12) + comp(10),
                (comp(11) - comp(12)) - -comp(10),
                (comp(11) - comp(12)) + comp(10),
                -(comp(12) - comp(11)) + comp(10),
            ],
            "#11 - #12 + #10",
        );

        assert_expr(
            &[
                (comp(11) + comp(12)) - (comp(10) + comp(13)),
                (comp(11) + comp(12)) + -(comp(10) + comp(13)),
                -(comp(10) + comp(13)) + (comp(11) + comp(12)),
                -(comp(10) + comp(13)) - -(comp(11) + comp(12)),
            ],
            "#11 + #12 - (#10 + #13)",
        );

        assert_expr(
            &[
                (comp(11) - comp(12)) - (comp(10) + comp(13)),
                (comp(11) - comp(12)) + -(comp(10) + comp(13)),
                -(comp(10) + comp(13)) + (comp(11) - comp(12)),
                -(comp(10) + comp(13)) - -(comp(11) - comp(12)),
            ],
            "#11 - #12 - (#10 + #13)",
        );

        assert_expr(
            &[(comp(11) + comp(12)) - (comp(10) - comp(13))],
            "#11 + #12 - (#10 - #13)",
        );
        assert_expr(
            &[(comp(11) + comp(12)) + -(comp(10) - comp(13))],
            "#11 + #12 + #13 - #10",
        );
        assert_expr(
            &[
                -(comp(10) - comp(13)) + (comp(11) + comp(12)),
                -(comp(10) - comp(13)) - -(comp(11) + comp(12)),
            ],
            "#13 - #10 + #11 + #12",
        );
    }

    #[test]
    fn test_functions() {
        let comp = Expr::component;
        let coalesce = Expr::coalesce;
        let number = Expr::number;

        assert_expr(
            &[
                comp(1) - (coalesce(comp(5), comp(7) + comp(6)) + coalesce(comp(2), comp(3)))
                    + coalesce(
                        number(0.0).max(comp(5)),
                        number(0.0).max(comp(7)) + number(0.0).max(comp(6)),
                    ),
            ],
            concat!(
                "#1 - (COALESCE(#5, #7 + #6) + COALESCE(#2, #3)) + ",
                "COALESCE(MAX(0.0, #5), MAX(0.0, #7) + MAX(0.0, #6))"
            ),
        );

        assert_expr(
            &[number(0.0).min(comp(5)).min(comp(7) + comp(6))
                - coalesce(comp(5), comp(7) + comp(6))
                    .max(comp(7))
                    .max(number(22.44))],
            "MIN(0.0, #5, #7 + #6) - MAX(COALESCE(#5, #7 + #6), #7, 22.44)",
        )
    }
}
