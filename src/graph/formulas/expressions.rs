// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

#![allow(dead_code)]

use crate::Node;
#[derive(Debug, Clone)]
pub(crate) enum FormulaExpression {
    Neg { param: Box<FormulaExpression> },
    Number { value: f64 },
    Component { component_id: u64 },
    Add { params: Vec<FormulaExpression> },
    Sub { params: Vec<FormulaExpression> },
    Coalesce { params: Vec<FormulaExpression> },
    Min { params: Vec<FormulaExpression> },
    Max { params: Vec<FormulaExpression> },
}

impl std::ops::Add for FormulaExpression {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        match (self, rhs) {
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

impl std::ops::Sub for FormulaExpression {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        match (self, rhs) {
            (sub @ Self::Sub { .. }, Self::Neg { param }) => sub + *param,
            (Self::Neg { param }, sub @ Self::Sub { .. }) => -sub - *param,
            (Self::Sub { mut params }, rhs) => {
                params.push(rhs);
                Self::Sub { params }
            }
            (Self::Neg { param: lhs }, Self::Neg { param: rhs }) => Self::Sub {
                params: vec![*rhs, *lhs],
            },
            (Self::Neg { param }, value) => Self::negative(*param + value),
            (lhs, Self::Neg { param }) => lhs + *param,
            (lhs, rhs) => Self::Sub {
                params: vec![lhs, rhs],
            },
        }
    }
}

impl std::ops::Neg for FormulaExpression {
    type Output = Self;

    fn neg(self) -> Self {
        Self::negative(self)
    }
}

impl<N: Node> From<&N> for FormulaExpression {
    fn from(node: &N) -> Self {
        Self::Component {
            component_id: node.component_id(),
        }
    }
}

/// Constructors for `FormulaExpression`.
impl FormulaExpression {
    pub(crate) fn negative(param: FormulaExpression) -> Self {
        if let Self::Neg { param: inner } = param {
            *inner
        } else if let Self::Sub { mut params } = param {
            let first = params.remove(0);
            Self::Add { params } - first
        } else {
            Self::Neg {
                param: Box::new(param),
            }
        }
    }

    pub(crate) fn number(value: f64) -> Self {
        Self::Number { value }
    }

    pub(crate) fn component(component_id: u64) -> Self {
        Self::Component { component_id }
    }

    pub(crate) fn components(component_ids: impl IntoIterator<Item = u64>) -> Vec<Self> {
        component_ids.into_iter().map(Self::component).collect()
    }

    pub(crate) fn add(params: Vec<FormulaExpression>) -> Self {
        Self::Add { params }
    }

    pub(crate) fn subtract(params: Vec<FormulaExpression>) -> Self {
        Self::Sub { params }
    }

    pub(crate) fn coalesce(params: Vec<FormulaExpression>) -> Self {
        Self::Coalesce { params }
    }

    pub(crate) fn min(params: Vec<FormulaExpression>) -> Self {
        Self::Min { params }
    }

    pub(crate) fn max(params: Vec<FormulaExpression>) -> Self {
        Self::Max { params }
    }
}

impl std::fmt::Display for FormulaExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.generate_string(false))
    }
}

#[derive(PartialEq)]
enum BracketComponents {
    First,
    Rest,
    All,
    None,
}

/// Display helpers for `FormulaExpression`.
impl FormulaExpression {
    fn join_params(
        params: &[FormulaExpression],
        separator: &str,
        prefix: Option<&str>,
        bracket_components: BracketComponents,
        bracket_whole: bool,
    ) -> String {
        let (mut result, suffix) = match prefix {
            Some(prefix) => (format!("{}(", prefix), String::from(")")),
            None => (String::new(), String::new()),
        };
        let mut num_components = 0;
        for expression in params.iter() {
            if num_components > 0 {
                result.push_str(separator);
            }
            if (bracket_components == BracketComponents::First && num_components == 0)
                || (bracket_components == BracketComponents::Rest && num_components > 0)
                || (bracket_components == BracketComponents::All)
            {
                result.push_str(&expression.generate_string(true));
            } else {
                result.push_str(&expression.generate_string(false));
            }
            num_components += 1;
        }
        if bracket_whole && num_components > 1 {
            String::from("(") + &result + &suffix + ")"
        } else {
            result + &suffix
        }
    }

    fn generate_string(&self, bracket_whole: bool) -> String {
        match self {
            Self::Neg { param } => format!("-{}", param.generate_string(true)),
            Self::Number { value } => value.to_string(),
            Self::Component { component_id } => format!("#{}", component_id),
            Self::Add { params } => {
                Self::join_params(params, " + ", None, BracketComponents::None, bracket_whole)
            }
            Self::Sub { params } => {
                Self::join_params(params, " - ", None, BracketComponents::Rest, bracket_whole)
            }
            Self::Coalesce { params } => Self::join_params(
                params,
                ", ",
                Some("COALESCE"),
                BracketComponents::None,
                false,
            ),
            Self::Min { params } => {
                Self::join_params(params, ", ", Some("MIN"), BracketComponents::None, false)
            }
            Self::Max { params } => {
                Self::join_params(params, ", ", Some("MAX"), BracketComponents::None, false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FormulaExpression;

    #[track_caller]
    fn assert_expr(exprs: &[FormulaExpression], expected: &str) {
        for expr in exprs {
            assert_eq!(expr.to_string(), expected);
        }
    }

    #[test]
    fn test_arithmatic() {
        let comp = FormulaExpression::component;

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
        let comp = FormulaExpression::component;
        let coalesce = FormulaExpression::coalesce;
        let number = FormulaExpression::number;
        let min = FormulaExpression::min;
        let max = FormulaExpression::max;

        assert_expr(
            &[comp(1)
                - (coalesce(vec![comp(5), comp(7) + comp(6)]) + coalesce(vec![comp(2), comp(3)]))
                + coalesce(vec![
                    max(vec![number(0.0), comp(5)]),
                    max(vec![number(0.0), comp(7)]) + max(vec![number(0.0), comp(6)]),
                ])],
            concat!(
                "#1 - (COALESCE(#5, #7 + #6) + COALESCE(#2, #3)) + ",
                "COALESCE(MAX(0, #5), MAX(0, #7) + MAX(0, #6))"
            ),
        );

        assert_expr(
            &[min(vec![number(0.0), comp(5), comp(7) + comp(6)])
                - max(vec![
                    coalesce(vec![comp(5), comp(7) + comp(6)]),
                    comp(7),
                    number(22.44),
                ])],
            "MIN(0, #5, #7 + #6) - MAX(COALESCE(#5, #7 + #6), #7, 22.44)",
        )
    }
}
