//! Linear, capped backoff (AR9) — matches the hub's own philosophy:
//! predictable, no cap-math surprises.

pub const STEP_SECS: u64 = 2;
pub const CAP_SECS: u64 = 60;

/// Delay before retry `attempt` (1-based). 1 → 2 s, 2 → 4 s, … 30+ → 60 s.
pub fn retry_delay_secs(attempt: u32) -> u64 {
    (STEP_SECS * u64::from(attempt.max(1))).min(CAP_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ar9_linear_then_capped() {
        assert_eq!(retry_delay_secs(1), 2);
        assert_eq!(retry_delay_secs(5), 10);
        assert_eq!(retry_delay_secs(30), 60);
        assert_eq!(retry_delay_secs(1000), 60);
    }

    #[test]
    fn ar9_attempt_zero_is_treated_as_one() {
        assert_eq!(retry_delay_secs(0), 2);
    }
}
