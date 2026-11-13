//! Explicit experimental exception, never the default campaign admission gate.
use super::{review_pressure_window, HostObservation, MlxRecipe, PreparationError, Reservation};
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalibrationPolicy {
    pub runtime_seconds: u32,
}
impl CalibrationPolicy {
    pub fn validate(&self) -> Result<(), PreparationError> {
        if !(30..=300).contains(&self.runtime_seconds) {
            return Err(PreparationError::InvalidRecipe);
        }
        Ok(())
    }
}

/// Trusted operator confirmation for a separately authorized calibration run.
/// Not serde: neither a model message nor readiness can construct this approval.
pub struct CalibrationApproval {
    recipe: MlxRecipe,
    policy: CalibrationPolicy,
    expires: Instant,
}
impl CalibrationApproval {
    pub(super) fn policy(&self) -> CalibrationPolicy {
        self.policy.clone()
    }
    pub fn confirm(
        recipe: &MlxRecipe,
        policy: &CalibrationPolicy,
        duration: Duration,
    ) -> Result<Self, PreparationError> {
        recipe.validate()?;
        policy.validate()?;
        if !(Duration::from_secs(1)..=Duration::from_secs(900)).contains(&duration) {
            return Err(PreparationError::InvalidRecipe);
        }
        Ok(Self {
            recipe: recipe.clone(),
            policy: policy.clone(),
            expires: Instant::now() + duration,
        })
    }
    pub(super) fn valid(&self, recipe: &MlxRecipe, policy: &CalibrationPolicy) -> bool {
        &self.recipe == recipe && &self.policy == policy && Instant::now() < self.expires
    }
}

#[derive(Debug, Serialize)]
pub struct CalibrationReadiness {
    pub pressure_review: super::AdmissionReview,
    pub reservation_verified: bool,
    pub eligible_for_experimental_calibration: bool,
    pub free_pages_threshold_waived_for_this_experiment: bool,
    pub capacity_guaranteed: bool,
    pub model_load_authorized: bool,
}

pub fn assess_calibration(
    recipe: &MlxRecipe,
    policy: &CalibrationPolicy,
    samples: &[HostObservation],
    now: u64,
    reservation: Option<&Reservation>,
) -> Result<CalibrationReadiness, PreparationError> {
    recipe.validate()?;
    policy.validate()?;
    let pressure_review = review_pressure_window(samples, now);
    let reservation_verified = reservation.is_some_and(|r| r.matches(recipe));
    let eligible = pressure_review.normal_pressure_observed && reservation_verified;
    Ok(CalibrationReadiness {
        pressure_review,
        reservation_verified,
        eligible_for_experimental_calibration: eligible,
        free_pages_threshold_waived_for_this_experiment: true,
        capacity_guaranteed: false,
        model_load_authorized: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_is_tied_to_recipe_policy_and_expiry_and_has_no_default_bypass() {
        let recipe = MlxRecipe {
            python: "/fixture/python".into(),
            runtime_entrypoint: "/fixture/entry".into(),
            checkpoint: "/fixture/model".into(),
            coordination_directory: "/fixture/shared".into(),
            minimum_free_bytes: 2 << 30,
        };
        let policy = CalibrationPolicy {
            runtime_seconds: 120,
        };
        let approval =
            CalibrationApproval::confirm(&recipe, &policy, Duration::from_secs(1)).unwrap();
        assert!(approval.valid(&recipe, &policy));
        assert!(!approval.valid(
            &recipe,
            &CalibrationPolicy {
                runtime_seconds: 60
            }
        ));
        let mut other = recipe.clone();
        other.checkpoint = "/fixture/other".into();
        assert!(!approval.valid(&other, &policy));
        let expired = CalibrationApproval {
            recipe: recipe.clone(),
            policy: policy.clone(),
            expires: Instant::now() - Duration::from_secs(1),
        };
        assert!(!expired.valid(&recipe, &policy));
        for runtime_seconds in [0, 29, 301, u32::MAX] {
            assert!(CalibrationPolicy { runtime_seconds }.validate().is_err());
        }
        assert!(CalibrationApproval::confirm(&recipe, &policy, Duration::from_secs(901)).is_err());
    }
}
