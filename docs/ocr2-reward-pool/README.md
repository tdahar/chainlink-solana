# OCR2 reward pool

The reward pool lets third parties ("sponsors", typically protocols that consume a feed) top up the
rewards of the oracles serving an OCR2 aggregator, on top of the aggregator's own billing. It lives
in the `ocr_2` program (`contracts/programs/ocr_2/src/rewards/`) and does not change the aggregator
`State` layout nor the `transmit` path.

## Accounts

| Account | Seeds | Purpose |
|---|---|---|
| `RewardPool` | `["reward_pool", state]` | One pool per aggregator: rate, admin, totals, reward index |
| Vault authority | `["reward_vault", pool]` | PDA owning the pool's LINK vault (associated token account) |
| `SponsorPosition` | `["sponsor", pool, sponsor]` | Deposits, withdrawals and pending withdrawal of one sponsor |
| `OracleRewardRecord` | `["oracle_reward", pool, transmitter]` | Accrued and claimed rewards of one oracle |

## Flow

1. **Setup.** The aggregator owner creates the pool vault (the associated token account of the vault
   authority PDA for the aggregator's LINK mint) and calls `init_reward_pool` with a reward per round
   and a sponsor withdrawal delay (at most 30 days). The owner becomes the pool admin; the admin role
   is transferred with `transfer_reward_pool_admin` / `accept_reward_pool_admin`.
2. **Sponsors** call `open_sponsor_position` once, then `deposit_rewards` any number of times.
3. **Oracles** that are part of the current aggregator config call `register_oracle`, signed by their
   transmitter key. Only registered oracles earn rewards, from the moment they register.
4. **Distribution.** For every round transmitted on the aggregator the pool releases
   `reward_per_round_gjuels`, split evenly between the registered oracles. Distribution is lazy: it
   runs at the start of every instruction that changes the pool, and anyone can crank it with
   `distribute_rewards`. Rounds transmitted before the pool existed, or while it is paused
   (`set_reward_pool_paused`), are not rewarded. When the pool runs out of unallocated balance the
   rounds are still marked as distributed and their rewards are forfeited.
5. **Claims.** An oracle's payee claims with `claim_oracle_rewards`; the instruction must be signed by
   the owner of the oracle's payee token account, the same rule as `withdraw_payment`.
6. **Config changes.** When an oracle leaves the aggregator, anyone can call `deregister_oracle` so it
   stops diluting the others. It keeps what it earned up to that point and can still claim it (to its
   last known payee). If it joins the aggregator again it calls `reactivate_oracle` and earns from
   that moment on.
7. **Sponsor withdrawals** take two steps. `request_sponsor_withdrawal` reserves an amount (no more
   than the sponsor's outstanding deposits and no more than the pool's unallocated balance) and
   starts the delay; the reserved amount is no longer released to oracles.
   `execute_sponsor_withdrawal` pays it out once the delay has passed, and
   `cancel_sponsor_withdrawal` releases the reservation.

## Accounting

The pool keeps a cumulative reward index per registered oracle, `acc_reward_per_oracle`, scaled by
`ACC_PRECISION` (1e12). Each `OracleRewardRecord` stores the index it was last settled at
(`reward_debt`); settling moves `(index - reward_debt) / ACC_PRECISION` into `accrued_gjuels`.
Distribution rounds the released amount down to a multiple of the number of registered oracles, so
`total_allocated_gjuels` is always exactly what the oracles can claim.

The pool's totals satisfy, at all times:

```
vault balance = total_deposited - total_claimed - total_withdrawn
unallocated   = total_deposited - total_allocated - total_withdrawn
distributable = unallocated - total_pending_withdrawal
```

## Maintenance

- `sweep_reward_dust` (pool admin) sends to a token account of the admin's choice the vault tokens the
  pool does not account for, i.e. tokens transferred to the vault directly instead of through
  `deposit_rewards`. Sponsor deposits, including pending withdrawals, and rewards owed to oracles are
  never swept.
- `rewards::query::pending_oracle_rewards` returns what an oracle could claim right now, counting the
  rounds not distributed yet, without changing any account.

Deposits are pooled: a sponsor's withdrawable amount is bounded by the pool's unallocated balance,
not by which sponsor's tokens were released to oracles.
