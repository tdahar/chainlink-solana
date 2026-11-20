# Operator staking

`operator-staking` (`contracts/programs/operator-staking`) lets the node operators of an OCR2 feed put LINK at
stake for the quality of the feed they serve, lets token holders delegate LINK to those operators for a share
of the rewards, and lets anyone slash operators when the feed goes stale.

It is a standalone program: it reads the feed through the `store` program's `query` CPI and does not change
`ocr_2` or `store`.

## Accounts

| Account | Seeds | Purpose |
|---|---|---|
| `StakingPool` | `["staking_pool", feed]` | One pool per feed: config, reward accumulator, totals |
| Vault authority | `["staking_vault", pool]` | PDA owning the pool's LINK vault (an associated token account) |
| `Operator` | `["operator", pool, authority]` | One per node operator: self-stake, delegated pool, rewards, unbonding queue |
| `Delegation` | `["delegation", operator, delegator]` | One per delegator and operator: shares, rewards, unbonding queue |

All tokens (stakes, unbonding tokens, the reward reserve and slashed tokens) sit in the one pool vault. The
pool's counters say what each part is:

```
vault balance >= total_staked + total_unbonding + reward_reserve + slashed_reserve + unclaimed rewards
```

(`>=` because reward emission rounds down and the dust stays in the vault.)

## Configuration

`PoolConfig`, set at `initialize_pool` and changed with `set_pool_config` (admin):

| Field | Meaning | Bounds |
|---|---|---|
| `min_operator_stake` | Minimum self-stake of an active operator | > 0 |
| `max_delegation_ratio` | Delegations an operator may accept, as a multiple of its self-stake | 1 to 100 |
| `unbonding_period_secs` | Time between an unstake request and the withdrawal | 0 to 60 days |
| `alert_staleness_secs` | Age of the feed's latest round after which an alert can be raised | > 0 |
| `slash_bps` | Share of an operator's total stake one alert slashes | at most 20% |
| `alerter_reward_bps` | Share of a slash paid to the alerter | at most 50% |

A config change applies to later actions only: unbonding requests keep the unlock time they were given.

The admin also sets the reward rate (`set_reward_rate`), pauses the pool (`set_paused`) and hands the role over
in two steps (`transfer_admin`, `accept_admin`). While paused, nothing is emitted, and no stake, delegation or
alert can be added; claims and withdrawals keep working.

## Operators

- `register_operator(commission_bps, stake)` creates the operator with its first self-stake, at least
  `min_operator_stake`. The commission is at most `MAX_COMMISSION_BPS` (50%).
- `set_commission(commission_bps)` changes the commission from now on; rewards earned before the change are
  split at the old commission. The same 50% cap applies.
- `add_operator_stake(amount)` adds self-stake, which also raises how much the operator can accept in
  delegations.
- `request_operator_unstake(amount)` moves self-stake to the unbonding queue. What remains must stay at or
  above `min_operator_stake` and back the current delegations (`delegated <= self_stake * max_delegation_ratio`).
  Unstaking everything is only allowed once every delegator has left, and deactivates the operator.
- `withdraw_operator_unbonded` pays the unlocked part of the queue; `claim_operator_rewards` pays the operator's
  rewards. Both pay to any token account of the pool's mint the operator authority chooses.

## Delegators

A delegator first calls `open_delegation` for an operator, then `delegate(amount)`.

Each operator keeps a delegated pool: `delegated_amount` tokens for `delegated_shares` shares. Delegating mints
shares at the current share price, `delegated_shares / delegated_amount` shares per token, as it is before the
deposit; the first delegation mints one share per token. Slashing lowers `delegated_amount` and not the share
count, so every delegator of the operator loses the same fraction.

- `request_undelegate(shares)` burns shares and moves the tokens they are worth into the delegation's unbonding
  queue.
- `withdraw_delegation_unbonded` pays the unlocked part of the queue; `claim_delegation_rewards` pays rewards.
- `compound_delegation_rewards` restakes the delegation's unclaimed rewards with the same operator: they join
  the operator's delegated pool for new shares at the current share price, count as staked from then on and
  earn rewards like any other stake. It needs the same capacity as a `delegate` of that amount.

`redelegate(shares)` moves a delegation to another operator of the same pool in one step: the tokens stay
staked, so there is no unbonding period and no gap in rewards. The delegator opens a delegation with the
destination operator first; the destination must be active and have capacity for the amount. Moving stake to
an operator of another pool goes through `request_undelegate` and a new `delegate`.

An operator or delegation has at most `MAX_UNBONDING_REQUESTS` (8) requests in flight. A request is unlocked
once `now >= unlock_ts` and is paid exactly once.

## Rewards

Anyone can top up the reward reserve with `fund_rewards`. The pool emits `reward_rate_per_sec` from the reserve,
pro rata to `total_staked`, through the accumulator `acc_reward_per_stake` (scaled by 1e12). Nothing is emitted
while nothing is staked or the pool is paused; emission stops when the reserve is empty.

Settling an operator splits what its total stake earned since it was last settled:

- the self-stake part goes to the operator;
- the delegated part goes to the delegators, minus the operator's commission, which goes to the operator.

The delegators' part grows the operator's `acc_reward_per_share`, and each delegation is paid per share. Every
instruction that changes a stake settles first, so rewards are paid on the stake that earned them.

## Alerts and slashing

`raise_alert` is permissionless. The alerter names an operator of the pool and passes the pool's feed; the
program reads the feed's latest round through the store program and, when it is older than
`alert_staleness_secs`, slashes `slash_bps` of the operator's self-stake and of its delegated pool. The alerter
receives `alerter_reward_bps` of the slash; the rest goes to `slashed_reserve`, which the admin can send to a
treasury with `withdraw_slashed`.

An operator is slashed at most once per feed round (`last_slashed_round_id`): a feed that stays stale on the
same round cannot slash the same operator again. Tokens already in an unbonding queue are not slashed.

## Read-only queries

`operator_staking::query` returns what the instructions would pay now, without changing any account:
`pool_summary`, `operator_summary` and `delegation_summary`.

## Testing

```sh
cd contracts
cargo test -p operator-staking --lib   # reward, share and slash arithmetic
anchor test                            # tests/operator-staking.spec.ts against a local validator
```

The integration test registers an operator and a delegator, checks that both earn rewards, slashes the operator
with a stale round written to a test feed, and walks the delegator and the operator out through the unbonding
queue.
