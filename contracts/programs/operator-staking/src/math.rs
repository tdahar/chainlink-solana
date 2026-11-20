//! Pure staking arithmetic, free of account types so it can be unit tested. Every function returns `None`
//! on overflow; callers map it to `StakingError::Overflow`.

use crate::state::{UnbondingRequest, BPS};

/// Scaling factor of the reward accumulators.
pub const PRECISION: u128 = 1_000_000_000_000;

/// Rewards released for `elapsed_secs` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Emission {
    /// Tokens moved from the reward reserve to stakers.
    pub emitted: u64,
    /// Increase of `acc_reward_per_stake`.
    pub acc_increase: u128,
}

/// Emits `rate_per_sec * elapsed_secs`, capped by the reserve, pro rata to `total_staked`. Nothing is
/// emitted while nothing is staked, so the reserve is not spent on an empty pool.
pub fn emission(
    rate_per_sec: u64,
    elapsed_secs: u64,
    reward_reserve: u64,
    total_staked: u64,
) -> Option<Emission> {
    if total_staked == 0 || rate_per_sec == 0 || elapsed_secs == 0 {
        return Some(Emission {
            emitted: 0,
            acc_increase: 0,
        });
    }
    let due = u128::from(rate_per_sec).checked_mul(u128::from(elapsed_secs))?;
    let emitted = u64::try_from(due.min(u128::from(reward_reserve))).ok()?;
    let acc_increase = u128::from(emitted).checked_mul(PRECISION)? / u128::from(total_staked);
    Some(Emission {
        emitted,
        acc_increase,
    })
}

/// Rewards earned by `amount` staked tokens (or shares) since the accumulator was at `debt`.
pub fn accrued(amount: u64, acc: u128, debt: u128) -> Option<u64> {
    let delta = acc.checked_sub(debt)?;
    let earned = u128::from(amount).checked_mul(delta)? / PRECISION;
    u64::try_from(earned).ok()
}

/// How an operator's rewards are split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardSplit {
    /// Self-stake rewards plus the commission on delegated rewards.
    pub operator: u64,
    /// Delegated rewards net of commission.
    pub delegators: u64,
}

/// Splits `rewards` earned by an operator's total stake between the operator and its delegators: the
/// self-stake part goes to the operator, the delegated part to the delegators minus `commission_bps`.
pub fn split_rewards(
    rewards: u64,
    self_stake: u64,
    delegated: u64,
    commission_bps: u16,
) -> Option<RewardSplit> {
    let total = self_stake.checked_add(delegated)?;
    if total == 0 {
        return Some(RewardSplit {
            operator: 0,
            delegators: 0,
        });
    }
    let delegated_rewards =
        u64::try_from(u128::from(rewards) * u128::from(delegated) / u128::from(total)).ok()?;
    let self_rewards = rewards.checked_sub(delegated_rewards)?;
    let commission = bps_of(delegated_rewards, commission_bps)?;
    Some(RewardSplit {
        operator: self_rewards.checked_add(commission)?,
        delegators: delegated_rewards.checked_sub(commission)?,
    })
}

/// Increase of an operator's per-share accumulator when `delegator_rewards` are spread over `shares`.
pub fn per_share_increase(delegator_rewards: u64, shares: u64) -> Option<u128> {
    if shares == 0 {
        return Some(0);
    }
    Some(u128::from(delegator_rewards).checked_mul(PRECISION)? / u128::from(shares))
}

/// Shares minted for depositing `amount` into a delegated pool holding `total_amount` tokens for
/// `total_shares` shares. The first deposit mints one share per token. A pool with shares but no tokens
/// left cannot price a deposit: `None`.
pub fn shares_for_amount(amount: u64, total_amount: u64, total_shares: u64) -> Option<u64> {
    if total_shares == 0 {
        return Some(amount);
    }
    if total_amount == 0 {
        return None;
    }
    let shares =
        u128::from(amount).checked_mul(u128::from(total_shares))? / u128::from(total_amount);
    u64::try_from(shares).ok()
}

/// Tokens `shares` are worth in a delegated pool holding `total_amount` tokens for `total_shares` shares.
pub fn amount_for_shares(shares: u64, total_amount: u64, total_shares: u64) -> Option<u64> {
    if total_shares == 0 {
        return Some(0);
    }
    let amount =
        u128::from(shares).checked_mul(u128::from(total_amount))? / u128::from(total_shares);
    u64::try_from(amount).ok()
}

/// `amount * bps / 10_000`, rounded down.
pub fn bps_of(amount: u64, bps: u16) -> Option<u64> {
    let v = u128::from(amount).checked_mul(u128::from(bps))? / u128::from(BPS);
    u64::try_from(v).ok()
}

/// How one alert slashes an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slash {
    /// Taken from the operator's self-stake.
    pub from_self: u64,
    /// Taken from its delegators (lowers the value of their shares).
    pub from_delegated: u64,
    /// Paid to the alerter.
    pub alerter_reward: u64,
    /// Kept by the pool in `slashed_reserve`.
    pub to_reserve: u64,
}

/// Slashes `slash_bps` of the operator's total stake, from self-stake and delegations in proportion,
/// and pays `alerter_reward_bps` of it to the alerter.
pub fn slash(
    self_stake: u64,
    delegated: u64,
    slash_bps: u16,
    alerter_reward_bps: u16,
) -> Option<Slash> {
    let from_self = bps_of(self_stake, slash_bps)?;
    let from_delegated = bps_of(delegated, slash_bps)?;
    let total = from_self.checked_add(from_delegated)?;
    let alerter_reward = bps_of(total, alerter_reward_bps)?;
    Some(Slash {
        from_self,
        from_delegated,
        alerter_reward,
        to_reserve: total.checked_sub(alerter_reward)?,
    })
}

/// Removes the requests of `queue` that are unlocked at `now` and returns the amount they hold.
pub fn take_unlocked(queue: &mut Vec<UnbondingRequest>, now: i64) -> Option<u64> {
    let unlocked = queue
        .iter()
        .filter(|request| request.unlock_ts <= now)
        .try_fold(0u64, |sum, request| sum.checked_add(request.amount))?;
    queue.retain(|request| request.unlock_ts >= now);
    Some(unlocked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(amount: u64, unlock_ts: i64) -> UnbondingRequest {
        UnbondingRequest { amount, unlock_ts }
    }

    #[test]
    fn emission_is_zero_without_stake() {
        let e = emission(10, 100, 1_000, 0).unwrap();
        assert_eq!(e.emitted, 0);
        assert_eq!(e.acc_increase, 0);
    }

    #[test]
    fn emission_is_zero_without_time() {
        assert_eq!(emission(10, 0, 1_000, 50).unwrap().emitted, 0);
    }

    #[test]
    fn emission_is_pro_rata() {
        let e = emission(10, 100, 10_000, 500).unwrap();
        assert_eq!(e.emitted, 1_000);
        assert_eq!(e.acc_increase, 2 * PRECISION);
    }

    #[test]
    fn emission_is_capped_by_reserve() {
        let e = emission(10, 100, 400, 100).unwrap();
        assert_eq!(e.emitted, 400);
        assert_eq!(e.acc_increase, 4 * PRECISION);
    }

    #[test]
    fn emission_does_not_overflow_on_long_gaps() {
        let e = emission(u64::MAX, u64::MAX, 7, 7).unwrap();
        assert_eq!(e.emitted, 7);
    }

    #[test]
    fn accrued_is_the_accumulator_delta() {
        assert_eq!(accrued(50, 3 * PRECISION, PRECISION), Some(100));
        assert_eq!(accrued(50, PRECISION, PRECISION), Some(0));
        assert_eq!(accrued(50, PRECISION, 2 * PRECISION), None);
    }

    #[test]
    fn split_without_delegations_goes_to_operator() {
        let s = split_rewards(1_000, 500, 0, 1_000).unwrap();
        assert_eq!(s.operator, 1_000);
        assert_eq!(s.delegators, 0);
    }

    #[test]
    fn split_takes_commission_on_delegated_part_only() {
        // 1/4 self-stake, 3/4 delegated, 10% commission
        let s = split_rewards(1_000, 100, 300, 1_000).unwrap();
        assert_eq!(s.operator, 250 + 75);
        assert_eq!(s.delegators, 675);
        assert_eq!(s.operator + s.delegators, 1_000);
    }

    #[test]
    fn split_of_empty_stake_is_zero() {
        let s = split_rewards(1_000, 0, 0, 1_000).unwrap();
        assert_eq!(s.operator + s.delegators, 0);
    }

    #[test]
    fn first_deposit_mints_one_share_per_token() {
        assert_eq!(shares_for_amount(123, 0, 0), Some(123));
    }

    #[test]
    fn shares_follow_the_share_price() {
        // 200 tokens for 100 shares: a share is worth 2 tokens
        assert_eq!(shares_for_amount(50, 200, 100), Some(25));
        assert_eq!(amount_for_shares(25, 200, 100), Some(50));
    }

    #[test]
    fn slashed_pool_mints_more_shares() {
        // 80 tokens left for 100 shares after a 20% slash
        assert_eq!(shares_for_amount(80, 80, 100), Some(100));
    }

    #[test]
    fn worthless_pool_cannot_price_a_deposit() {
        assert_eq!(shares_for_amount(10, 0, 100), None);
    }

    #[test]
    fn amount_for_shares_without_shares_is_zero() {
        assert_eq!(amount_for_shares(10, 0, 0), Some(0));
    }

    #[test]
    fn slash_is_proportional() {
        let s = slash(1_000, 3_000, 1_000, 2_500).unwrap();
        assert_eq!(s.from_self, 100);
        assert_eq!(s.from_delegated, 300);
        assert_eq!(s.alerter_reward, 100);
        assert_eq!(s.to_reserve, 300);
    }

    #[test]
    fn bps_rounds_down() {
        assert_eq!(bps_of(999, 1), Some(0));
        assert_eq!(bps_of(10_000, 1), Some(1));
    }

    #[test]
    fn take_unlocked_keeps_locked_requests() {
        let mut queue = vec![request(10, 100), request(20, 300), request(30, 200)];
        assert_eq!(take_unlocked(&mut queue, 250), Some(40));
        assert_eq!(queue, vec![request(20, 300)]);
    }

    #[test]
    fn take_unlocked_with_nothing_unlocked() {
        let mut queue = vec![request(10, 100)];
        assert_eq!(take_unlocked(&mut queue, 50), Some(0));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn take_unlocked_empties_the_queue() {
        let mut queue = vec![request(10, 100), request(20, 150)];
        assert_eq!(take_unlocked(&mut queue, 1_000), Some(30));
        assert!(queue.is_empty());
    }

    #[test]
    fn take_unlocked_twice_pays_once() {
        let mut queue = vec![request(10, 100), request(20, 300)];
        assert_eq!(take_unlocked(&mut queue, 200), Some(10));
        assert_eq!(take_unlocked(&mut queue, 250), Some(0));
        assert_eq!(take_unlocked(&mut queue, 400), Some(20));
        assert!(queue.is_empty());
    }

    // -- Conservation properties over a grid of inputs

    const AMOUNTS: [u64; 7] = [0, 1, 7, 999, 10_000, 1_234_567, 1_000_000_000_000];
    const BPS_VALUES: [u16; 6] = [0, 1, 500, 1_000, 2_500, 5_000];

    #[test]
    fn split_never_creates_or_loses_rewards() {
        for rewards in AMOUNTS {
            for self_stake in AMOUNTS {
                for delegated in AMOUNTS {
                    for commission in BPS_VALUES {
                        let s = split_rewards(rewards, self_stake, delegated, commission).unwrap();
                        if self_stake == 0 && delegated == 0 {
                            assert_eq!(s.operator + s.delegators, 0);
                        } else {
                            assert_eq!(s.operator + s.delegators, rewards);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn split_gives_delegators_nothing_without_delegations() {
        for rewards in AMOUNTS {
            for commission in BPS_VALUES {
                let s = split_rewards(rewards, 1_000, 0, commission).unwrap();
                assert_eq!(s.delegators, 0);
            }
        }
    }

    #[test]
    fn slash_never_creates_or_loses_tokens() {
        for self_stake in AMOUNTS {
            for delegated in AMOUNTS {
                for slash_bps in [0u16, 1, 1_000, 2_000] {
                    for alerter_bps in BPS_VALUES {
                        let s = slash(self_stake, delegated, slash_bps, alerter_bps).unwrap();
                        assert!(s.from_self <= self_stake);
                        assert!(s.from_delegated <= delegated);
                        assert_eq!(
                            s.alerter_reward + s.to_reserve,
                            s.from_self + s.from_delegated
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shares_round_trip_never_pays_more_than_deposited() {
        for total_amount in AMOUNTS {
            for total_shares in AMOUNTS {
                // Tokens without shares cannot happen: the last undelegation takes every token.
                if total_shares == 0 && total_amount > 0 {
                    continue;
                }
                for amount in AMOUNTS {
                    let Some(shares) = shares_for_amount(amount, total_amount, total_shares) else {
                        continue;
                    };
                    let new_amount = total_amount.saturating_add(amount);
                    let new_shares = total_shares.saturating_add(shares);
                    if let Some(back) = amount_for_shares(shares, new_amount, new_shares) {
                        assert!(back <= amount, "{amount} in, {back} out");
                    }
                }
            }
        }
    }

    #[test]
    fn emission_never_exceeds_reserve() {
        for rate in AMOUNTS {
            for elapsed in [0u64, 1, 60, 86_400, u64::MAX] {
                for reserve in AMOUNTS {
                    let e = emission(rate, elapsed, reserve, 1_000).unwrap();
                    assert!(e.emitted <= reserve);
                }
            }
        }
    }

    #[test]
    fn accrued_over_split_stake_is_at_most_the_whole() {
        let acc = 7 * PRECISION + 123_456;
        for a in AMOUNTS {
            for b in AMOUNTS {
                let whole = accrued(a + b, acc, 0).unwrap();
                let parts = accrued(a, acc, 0).unwrap() + accrued(b, acc, 0).unwrap();
                assert!(parts <= whole);
                assert!(whole - parts <= 1);
            }
        }
    }
}
