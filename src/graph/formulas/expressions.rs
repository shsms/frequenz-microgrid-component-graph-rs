// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

#![allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum FormulaExpression {
    Number { value: f64 },
    Component { component_id: u64 },
    Add { params: Vec<FormulaExpression> },
    Subtract { params: Vec<FormulaExpression> },
    Coalesce { params: Vec<FormulaExpression> },
    Min { params: Vec<FormulaExpression> },
    Max { params: Vec<FormulaExpression> },
}

impl std::fmt::Display for FormulaExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut formula = self.generate_string();
        if formula.starts_with('(') && formula.ends_with(')') {
            formula = formula[1..formula.len() - 1].to_string();
        }
        write!(f, "{}", formula)
    }
}

fn join_params(params: &[FormulaExpression], separator: &str, prefix: Option<&str>) -> String {
    let mut result = String::from(prefix.unwrap_or_default()) + "(";
    for (i, expression) in params.iter().enumerate() {
        if i > 0 {
            result.push_str(separator);
        }
        result.push_str(&expression.generate_string());
    }
    result + ")"
}

impl FormulaExpression {
    pub(crate) fn number(value: f64) -> Self {
        Self::Number { value }
    }

    pub(crate) fn component(component_id: u64) -> Self {
        Self::Component { component_id }
    }

    pub(crate) fn components(component_ids: impl IntoIterator<Item = u64>) -> Vec<Self> {
        component_ids
            .into_iter()
            .map(|id| Self::component(id))
            .collect()
    }

    pub(crate) fn add(params: Vec<FormulaExpression>) -> Self {
        Self::Add { params }
    }

    pub(crate) fn subtract(params: Vec<FormulaExpression>) -> Self {
        Self::Subtract { params }
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

    fn generate_string(&self) -> String {
        match self {
            Self::Number { value } => value.to_string(),
            Self::Component { component_id } => format!("#{}", component_id),
            Self::Add { params } => join_params(params, " + ", None),
            Self::Subtract { params } => join_params(params, " - ", None),
            Self::Coalesce { params } => join_params(params, ", ", Some("COALESCE")),
            Self::Min { params } => join_params(params, ", ", Some("MIN")),
            Self::Max { params } => join_params(params, ", ", Some("MAX")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expression_as_string() {
        let expr = FormulaExpression::Add {
            params: vec![
                FormulaExpression::Component { component_id: 1 },
                FormulaExpression::Subtract {
                    params: vec![
                        FormulaExpression::Number { value: 2.0 },
                        FormulaExpression::Component { component_id: 3 },
                    ],
                },
                FormulaExpression::Max {
                    params: vec![
                        FormulaExpression::Number { value: 0.0 },
                        FormulaExpression::Component { component_id: 4 },
                    ],
                },
            ],
        };
        assert_eq!(expr.to_string(), "#1 + (2 - #3) + MAX(0, #4)");
    }
}
