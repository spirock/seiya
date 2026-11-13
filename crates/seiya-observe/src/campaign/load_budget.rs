//! Planned disjoint categories, never a sum of overlapping RSS/Metal/footprint.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Estimate,
    Measured,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetAmount {
    pub bytes: u64,
    pub basis: Basis,
    /// Operator's evidence reference, not model text, a command or a filename to open.
    pub evidence: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadBudget {
    pub manifest_sha256: String,
    pub context_tokens: u32,
    pub output_tokens: u32,
    pub resident_weights: Option<BudgetAmount>,
    pub active_kv: Option<BudgetAmount>,
    pub transient_activations: Option<BudgetAmount>,
    pub disposable_caches: Option<BudgetAmount>,
    pub runtime_overhead: Option<BudgetAmount>,
    pub coordinator: Option<BudgetAmount>,
    pub host_margin: Option<BudgetAmount>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum BudgetError {
    InvalidIdentity,
    InvalidContext,
    InvalidAmount,
    Overflow,
}

#[derive(Debug, Serialize)]
pub struct BudgetReport {
    pub method: &'static str,
    pub missing: Vec<&'static str>,
    pub estimated_categories: Vec<&'static str>,
    pub planned_runtime_bytes: Option<u64>,
    pub planned_runtime_and_host_margin_bytes: Option<u64>,
    pub is_measured_peak: bool,
    pub safe_available_host_bytes: Option<u64>,
    pub model_load_authorized: bool,
}

/// Logical unquantized K/V shape only: no allocator padding, copies, temporary
/// tensors, sharing discovery or sliding-window inference is done here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KvLayerShape {
    pub kv_heads: u32,
    pub key_head_dim: u32,
    pub value_head_dim: u32,
    pub scalar_bytes: u32,
}

pub fn logical_kv_bytes(
    layers: &[KvLayerShape],
    tokens: u32,
    batch: u32,
) -> Result<u64, BudgetError> {
    if layers.is_empty()
        || layers.len() > 256
        || !(1..=1_048_576).contains(&tokens)
        || !(1..=16).contains(&batch)
    {
        return Err(BudgetError::InvalidContext);
    }
    let mut bytes_per_token = 0u64;
    for layer in layers {
        if !(1..=256).contains(&layer.kv_heads)
            || !(1..=4096).contains(&layer.key_head_dim)
            || !(1..=4096).contains(&layer.value_head_dim)
            || ![1, 2, 4, 8].contains(&layer.scalar_bytes)
        {
            return Err(BudgetError::InvalidAmount);
        }
        let bytes = u64::from(layer.kv_heads)
            .checked_mul(u64::from(layer.key_head_dim) + u64::from(layer.value_head_dim))
            .and_then(|n| n.checked_mul(u64::from(layer.scalar_bytes)))
            .ok_or(BudgetError::Overflow)?;
        bytes_per_token = bytes_per_token
            .checked_add(bytes)
            .ok_or(BudgetError::Overflow)?;
    }
    bytes_per_token
        .checked_mul(u64::from(tokens))
        .and_then(|n| n.checked_mul(u64::from(batch)))
        .ok_or(BudgetError::Overflow)
}

impl LoadBudget {
    pub fn evaluate(&self) -> Result<BudgetReport, BudgetError> {
        if self.manifest_sha256.len() != 64
            || !self
                .manifest_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(BudgetError::InvalidIdentity);
        }
        if self.context_tokens == 0
            || self.context_tokens > 1_048_576
            || self.output_tokens == 0
            || self.output_tokens > 4096
            || self.output_tokens >= self.context_tokens
        {
            return Err(BudgetError::InvalidContext);
        }
        let mut report = BudgetReport {
            method: "planned-disjoint-categories-not-observed-rss",
            missing: vec![],
            estimated_categories: vec![],
            planned_runtime_bytes: None,
            planned_runtime_and_host_margin_bytes: None,
            is_measured_peak: false,
            safe_available_host_bytes: None,
            model_load_authorized: false,
        };
        let mut runtime = 0u64;
        let categories = [
            ("resident_weights", &self.resident_weights),
            ("active_kv", &self.active_kv),
            ("transient_activations", &self.transient_activations),
            ("disposable_caches", &self.disposable_caches),
            ("runtime_overhead", &self.runtime_overhead),
            ("coordinator", &self.coordinator),
            ("host_margin", &self.host_margin),
        ];
        for (name, amount) in categories {
            let Some(amount) = amount else {
                report.missing.push(name);
                continue;
            };
            if amount.bytes == 0
                || amount.bytes > 1 << 40
                || amount.evidence.is_empty()
                || amount.evidence.len() > 256
                || amount.evidence.chars().any(char::is_control)
            {
                return Err(BudgetError::InvalidAmount);
            }
            if matches!(amount.basis, Basis::Estimate) {
                report.estimated_categories.push(name);
            }
            if name != "host_margin" {
                runtime = runtime
                    .checked_add(amount.bytes)
                    .ok_or(BudgetError::Overflow)?;
            }
        }
        if report.missing.iter().all(|name| *name == "host_margin") {
            report.planned_runtime_bytes = Some(runtime);
            if let Some(margin) = &self.host_margin {
                report.planned_runtime_and_host_margin_bytes = Some(
                    runtime
                        .checked_add(margin.bytes)
                        .ok_or(BudgetError::Overflow)?,
                );
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn budget() -> LoadBudget {
        let amount = || {
            Some(BudgetAmount {
                bytes: 100,
                basis: Basis::Estimate,
                evidence: "synthetic-test-only".into(),
            })
        };
        LoadBudget {
            manifest_sha256: "0123456789abcdef".repeat(4),
            context_tokens: 2048,
            output_tokens: 256,
            resident_weights: amount(),
            active_kv: amount(),
            transient_activations: amount(),
            disposable_caches: amount(),
            runtime_overhead: amount(),
            coordinator: amount(),
            host_margin: amount(),
        }
    }
    #[test]
    fn complete_estimate_is_not_peak_or_authorization() {
        let report = budget().evaluate().unwrap();
        assert_eq!(report.planned_runtime_bytes, Some(600));
        assert_eq!(report.planned_runtime_and_host_margin_bytes, Some(700));
        assert_eq!(report.estimated_categories.len(), 7);
        assert!(!report.is_measured_peak && !report.model_load_authorized);
        assert_eq!(report.safe_available_host_bytes, None);
    }
    #[test]
    fn missing_components_are_not_zero_or_a_partial_total() {
        let mut budget = budget();
        budget.active_kv = None;
        let report = budget.evaluate().unwrap();
        assert_eq!(report.missing, vec!["active_kv"]);
        assert_eq!(report.planned_runtime_bytes, None);
        let mut budget = super::tests::budget();
        budget.host_margin = None;
        let report = budget.evaluate().unwrap();
        assert_eq!(report.planned_runtime_bytes, Some(600));
        assert_eq!(report.planned_runtime_and_host_margin_bytes, None);
    }
    #[test]
    fn invalid_identity_context_amount_and_claims_fail_closed() {
        let mut budget = budget();
        budget.manifest_sha256 = "not-a-hash".into();
        assert!(matches!(
            budget.evaluate(),
            Err(BudgetError::InvalidIdentity)
        ));
        let mut budget = super::tests::budget();
        budget.context_tokens = 128;
        assert!(matches!(
            budget.evaluate(),
            Err(BudgetError::InvalidContext)
        ));
        for bytes in [0, (1 << 40) + 1, u64::MAX] {
            let mut budget = super::tests::budget();
            budget.active_kv.as_mut().unwrap().bytes = bytes;
            assert!(matches!(budget.evaluate(), Err(BudgetError::InvalidAmount)));
        }
        let mut json = serde_json::to_value(super::tests::budget()).unwrap();
        json["rss_plus_metal"] = 1.into();
        assert!(serde_json::from_value::<LoadBudget>(json).is_err());
    }

    #[test]
    fn logical_kv_shape_separates_heads_dimensions_dtype_batch_and_tokens() {
        let layer = KvLayerShape {
            kv_heads: 2,
            key_head_dim: 256,
            value_head_dim: 512,
            scalar_bytes: 2,
        };
        assert_eq!(
            logical_kv_bytes(std::slice::from_ref(&layer), 10, 1).unwrap(),
            30720
        );
        assert_eq!(
            logical_kv_bytes(&[layer.clone(), layer.clone()], 10, 2).unwrap(),
            122880
        );
        assert!(logical_kv_bytes(&[], 10, 1).is_err());
        assert!(logical_kv_bytes(std::slice::from_ref(&layer), 0, 1).is_err());
        let mut invalid = layer;
        invalid.scalar_bytes = 0;
        assert!(logical_kv_bytes(&[invalid], 10, 1).is_err());
    }
}
