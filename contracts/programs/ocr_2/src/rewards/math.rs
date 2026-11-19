//! Pure reward accounting helpers, kept free of account types so they can be unit tested.

/// Scaling factor applied to `acc_reward_per_oracle` to keep precision when splitting a round's
/// rewards between oracles.
pub const ACC_PRECISION: u128 = 1_000_000_000_000;

/// Result of releasing rewards for a range of rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Distribution {
    /// Amount moved from unallocated deposits to oracle rewards.
    pub allocated_gjuels: u64,
    /// Increase of the cumulative per-oracle index.
    pub acc_increase: u128,
}

/// Computes how much to release for `rounds` new rounds.
///
/// The release is capped by `distributable_gjuels` so the pool never promises more than it holds.
/// The amount is rounded down to a multiple that splits exactly between `registered_oracles`, so
/// that `allocated_gjuels` always equals what oracles can claim and no dust is lost.
pub fn distribution(
    reward_per_round_gjuels: u64,
    rounds: u32,
    registered_oracles: u32,
    distributable_gjuels: u64,
) -> Option<Distribution> {
    if registered_oracles == 0 || rounds == 0 || reward_per_round_gjuels == 0 {
        return Some(Distribution {
            allocated_gjuels: 0,
            acc_increase: 0,
        });
    }

    let due = reward_per_round_gjuels
        .checked_mul(u64::from(rounds))?
        .min(distributable_gjuels);

    let per_oracle = due / u64::from(registered_oracles);
    let allocated_gjuels = per_oracle.checked_mul(u64::from(registered_oracles))?;
    let acc_increase = u128::from(per_oracle).checked_mul(ACC_PRECISION)?;

    Some(Distribution {
        allocated_gjuels,
        acc_increase,
    })
}

/// Rewards accrued by an oracle since it was last settled at `reward_debt`.
pub fn pending_reward(acc_reward_per_oracle: u128, reward_debt: u128) -> Option<u64> {
    let pending = acc_reward_per_oracle.checked_sub(reward_debt)? / ACC_PRECISION;
    u64::try_from(pending).ok()
}

/// Amount a sponsor can request back: bounded by what it still has in the pool and by what the
/// pool has not released to oracles yet.
pub fn withdrawable(sponsor_outstanding_gjuels: u64, pool_distributable_gjuels: u64) -> u64 {
    sponsor_outstanding_gjuels.min(pool_distributable_gjuels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_without_oracles_releases_nothing() {
        let d = distribution(100, 10, 0, 1_000).unwrap();
        assert_eq!(d.allocated_gjuels, 0);
        assert_eq!(d.acc_increase, 0);
    }

    #[test]
    fn distribution_without_rounds_releases_nothing() {
        let d = distribution(100, 0, 4, 1_000).unwrap();
        assert_eq!(d.allocated_gjuels, 0);
        assert_eq!(d.acc_increase, 0);
    }

    #[test]
    fn distribution_splits_evenly() {
        let d = distribution(100, 3, 4, 10_000).unwrap();
        assert_eq!(d.allocated_gjuels, 300);
        assert_eq!(d.acc_increase, 75 * ACC_PRECISION);
    }

    #[test]
    fn distribution_rounds_down_to_split() {
        // 100 / 3 = 33 per oracle, 1 gjuel stays unallocated
        let d = distribution(100, 1, 3, 10_000).unwrap();
        assert_eq!(d.allocated_gjuels, 99);
        assert_eq!(d.acc_increase, 33 * ACC_PRECISION);
    }

    #[test]
    fn distribution_is_capped_by_balance() {
        let d = distribution(100, 50, 2, 1_001).unwrap();
        assert_eq!(d.allocated_gjuels, 1_000);
        assert_eq!(d.acc_increase, 500 * ACC_PRECISION);
    }

    #[test]
    fn distribution_overflow_is_reported() {
        assert!(distribution(u64::MAX, 2, 1, u64::MAX).is_none());
    }

    #[test]
    fn pending_reward_is_the_index_delta() {
        assert_eq!(
            pending_reward(150 * ACC_PRECISION, 100 * ACC_PRECISION),
            Some(50)
        );
        assert_eq!(
            pending_reward(100 * ACC_PRECISION, 100 * ACC_PRECISION),
            Some(0)
        );
    }

    #[test]
    fn pending_reward_rejects_debt_above_index() {
        assert_eq!(pending_reward(ACC_PRECISION, 2 * ACC_PRECISION), None);
    }

    #[test]
    fn allocated_matches_claimable() {
        // Whatever is allocated must be exactly claimable by the registered oracles.
        for oracles in 1..=19u32 {
            for rounds in [1u32, 7, 1_000] {
                let d = distribution(12_345, rounds, oracles, u64::MAX).unwrap();
                let per_oracle = pending_reward(d.acc_increase, 0).unwrap();
                assert_eq!(per_oracle * u64::from(oracles), d.allocated_gjuels);
            }
        }
    }

    #[test]
    fn withdrawable_is_bounded_by_both_sides() {
        assert_eq!(withdrawable(100, 1_000), 100);
        assert_eq!(withdrawable(1_000, 100), 100);
        assert_eq!(withdrawable(0, 100), 0);
    }
}
