//! Automatic switching strategies. Pure functions over a snapshot of usage so they can be unit-tested.
//!
//! Terminology:
//! * *binding usage* of an account = the higher of its 5-hour and 7-day readings.
//! * *exhausted* = binding usage >= the hard threshold ("Switch once usage reaches").
//! * A *Hold* never blocks a switch away from an exhausted account; it only blocks being
//!   pre-empted by a strategy that prefers a different account while the active one still has room.

use crate::model::Strategy;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub id: String,
    /// Credentials are valid (not "Needs login" / "No backup").
    pub usable: bool,
    pub five: Option<f64>,
    pub seven: Option<f64>,
    pub five_reset: Option<i64>,
    pub seven_reset: Option<i64>,
}

impl Candidate {
    pub fn binding(&self) -> f64 {
        self.five.unwrap_or(0.0).max(self.seven.unwrap_or(0.0))
    }
}

pub struct Input<'a> {
    /// In the user's own order (priority list for the "order" strategy).
    pub accounts: &'a [Candidate],
    pub active: Option<&'a str>,
    pub threshold: f64,
    pub strategy: Strategy,
    pub round_robin_step: f64,
    pub rr_lap: u32,
    pub hold_active: bool,
}

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub target: String,
    pub exhausted: bool,
    pub new_lap: u32,
}

pub fn decide(input: &Input) -> Option<Decision> {
    let th = input.threshold;
    let active_idx = input
        .active
        .and_then(|id| input.accounts.iter().position(|a| a.id == id));
    let active = active_idx.map(|i| &input.accounts[i]);
    let active_exhausted = match active {
        Some(a) => !a.usable || a.binding() >= th,
        None => true,
    };
    let lap = input.rr_lap.max(1);

    let eligible = |c: &Candidate| c.usable && c.binding() < th && Some(c.id.as_str()) != input.active;

    // A strategy that wants to move although the active account still has room is pre-empting.
    let blocked_by_hold = |exhausted: bool| input.hold_active && !exhausted;

    match input.strategy {
        Strategy::Headroom => {
            if !active_exhausted {
                return None;
            }
            input
                .accounts
                .iter()
                .filter(|c| eligible(c))
                .min_by(|a, b| a.binding().total_cmp(&b.binding()))
                .map(|c| Decision { target: c.id.clone(), exhausted: true, new_lap: lap })
        }
        Strategy::Order => {
            let desired = input.accounts.iter().find(|c| c.usable && c.binding() < th)?;
            if Some(desired.id.as_str()) == input.active {
                return None;
            }
            if blocked_by_hold(active_exhausted) {
                return None;
            }
            Some(Decision { target: desired.id.clone(), exhausted: active_exhausted, new_lap: lap })
        }
        Strategy::Balanced => {
            // Spend the account whose *weekly* window resets soonest: whatever is left in it is
            // wasted at the reset, while the others keep their quota for later. Weekly resets within
            // WEEK_BUCKET_MS of each other count as a tie, then the soonest 5-hour reset decides
            // (it favours the account whose session is already running), then list order.
            let mut ranked: Vec<(usize, &Candidate)> = input
                .accounts
                .iter()
                .enumerate()
                .filter(|(_, c)| c.usable && c.binding() < th)
                .collect();
            ranked.sort_by(|(ia, a), (ib, b)| {
                cmp_reset(a.seven_reset.map(week_bucket), b.seven_reset.map(week_bucket))
                    .then_with(|| cmp_reset(a.five_reset, b.five_reset))
                    .then_with(|| ia.cmp(ib))
            });
            let top = ranked.first()?.1;
            if Some(top.id.as_str()) == input.active {
                return None;
            }
            if blocked_by_hold(active_exhausted) {
                return None;
            }
            Some(Decision { target: top.id.clone(), exhausted: active_exhausted, new_lap: lap })
        }
        Strategy::RoundRobin => {
            let n = input.accounts.len();
            if n == 0 {
                return None;
            }
            let ceiling = |l: u32| (l as f64 * input.round_robin_step).min(th);
            if let Some(a) = active {
                if !active_exhausted && a.binding() < ceiling(lap) {
                    return None;
                }
            }
            if blocked_by_hold(active_exhausted) {
                return None;
            }
            let start = active_idx.map(|i| i + 1).unwrap_or(0);
            // Rotation order starting right after the active account (wrapping around).
            let rotation: Vec<(usize, &Candidate)> = (0..n)
                .map(|k| (start + k) % n)
                .map(|i| (i, &input.accounts[i]))
                .collect();
            // If nobody has room under the current lap's ceiling, start another lap.
            let max_laps = (th / input.round_robin_step).ceil() as u32 + 1;
            for l in lap..=lap + max_laps {
                for (i, c) in &rotation {
                    if !eligible(c) {
                        continue;
                    }
                    let wrapped = active_idx.is_some_and(|ai| *i <= ai);
                    let this_lap = if wrapped && l == lap { l + 1 } else { l };
                    if c.binding() < ceiling(this_lap) {
                        return Some(Decision {
                            target: c.id.clone(),
                            exhausted: active_exhausted,
                            new_lap: this_lap,
                        });
                    }
                }
            }
            None
        }
    }
}

/// Weekly resets closer than this are treated as the same moment (a total order, unlike a tolerance).
const WEEK_BUCKET_MS: i64 = 6 * 3_600_000;

fn week_bucket(reset_ms: i64) -> i64 {
    reset_ms.div_euclid(WEEK_BUCKET_MS)
}

fn cmp_reset(a: Option<i64>, b: Option<i64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(id: &str, five: f64, seven: f64, fr: Option<i64>, sr: Option<i64>) -> Candidate {
        Candidate {
            id: id.into(),
            usable: true,
            five: Some(five),
            seven: Some(seven),
            five_reset: fr,
            seven_reset: sr,
        }
    }

    fn input<'a>(accounts: &'a [Candidate], active: &'a str, strategy: Strategy) -> Input<'a> {
        Input {
            accounts,
            active: Some(active),
            threshold: 90.0,
            strategy,
            round_robin_step: 40.0,
            rr_lap: 1,
            hold_active: false,
        }
    }

    #[test]
    fn headroom_only_moves_when_exhausted_and_picks_lowest() {
        let a = [acc("a", 50.0, 20.0, None, None), acc("b", 30.0, 60.0, None, None), acc("c", 10.0, 10.0, None, None)];
        assert_eq!(decide(&input(&a, "a", Strategy::Headroom)), None);
        let a = [acc("a", 95.0, 20.0, None, None), acc("b", 30.0, 60.0, None, None), acc("c", 10.0, 10.0, None, None)];
        assert_eq!(decide(&input(&a, "a", Strategy::Headroom)).unwrap().target, "c");
    }

    #[test]
    fn order_returns_to_higher_priority_account() {
        let a = [acc("a", 20.0, 20.0, None, None), acc("b", 10.0, 10.0, None, None)];
        let d = decide(&input(&a, "b", Strategy::Order)).unwrap();
        assert_eq!(d.target, "a");
        assert!(!d.exhausted);
    }

    #[test]
    fn order_skips_exhausted_accounts() {
        let a = [acc("a", 95.0, 20.0, None, None), acc("b", 10.0, 10.0, None, None), acc("c", 0.0, 0.0, None, None)];
        assert_eq!(decide(&input(&a, "a", Strategy::Order)).unwrap().target, "b");
    }

    #[test]
    fn hold_blocks_preemption_but_not_exhaustion() {
        let a = [acc("a", 20.0, 20.0, None, None), acc("b", 10.0, 10.0, None, None)];
        let mut i = input(&a, "b", Strategy::Order);
        i.hold_active = true;
        assert_eq!(decide(&i), None);
        let a = [acc("a", 20.0, 20.0, None, None), acc("b", 95.0, 10.0, None, None)];
        let mut i = input(&a, "b", Strategy::Order);
        i.hold_active = true;
        assert_eq!(decide(&i).unwrap().target, "a");
    }

    const H: i64 = 3_600_000;

    #[test]
    fn balanced_prefers_soonest_weekly_reset_even_over_sooner_five_hour_reset() {
        // the situation from real use: unplex (active) 5h resets first, but volodya's week resets in 3d18h
        let a = [
            acc("unplex", 17.0, 2.0, Some(3 * H), Some(161 * H)),
            acc("dev", 0.0, 0.0, None, Some(160 * H)),
            acc("volodya", 2.0, 39.0, Some(5 * H), Some(90 * H)),
        ];
        assert_eq!(decide(&input(&a, "unplex", Strategy::Balanced)).unwrap().target, "volodya");
        assert_eq!(decide(&input(&a, "volodya", Strategy::Balanced)), None);
    }

    #[test]
    fn balanced_treats_close_weekly_resets_as_a_tie_and_uses_five_hour_reset() {
        // 1h apart is the same bucket as far as we care: the account whose session is running wins
        let a = [acc("a", 10.0, 10.0, Some(2 * H), Some(160 * H)), acc("b", 0.0, 0.0, None, Some(159 * H + 1))];
        assert_eq!(decide(&input(&a, "b", Strategy::Balanced)).unwrap().target, "a");
    }

    #[test]
    fn balanced_moves_on_to_the_next_week_when_the_best_is_used_up() {
        let a = [
            acc("a", 10.0, 10.0, Some(2 * H), Some(160 * H)),
            acc("b", 50.0, 95.0, Some(1 * H), Some(20 * H)), // soonest week but past the threshold
            acc("c", 5.0, 5.0, Some(4 * H), Some(100 * H)),
        ];
        assert_eq!(decide(&input(&a, "a", Strategy::Balanced)).unwrap().target, "c");
    }

    #[test]
    fn balanced_never_picks_account_past_threshold() {
        let a = [acc("a", 10.0, 10.0, Some(300), None), acc("b", 95.0, 10.0, Some(100), None)];
        assert_eq!(decide(&input(&a, "a", Strategy::Balanced)), None);
    }

    #[test]
    fn balanced_unknown_resets_rank_last() {
        let a = [acc("a", 10.0, 10.0, None, None), acc("b", 5.0, 5.0, Some(50), None)];
        assert_eq!(decide(&input(&a, "a", Strategy::Balanced)).unwrap().target, "b");
    }

    #[test]
    fn round_robin_stays_until_ceiling_then_rotates() {
        let a = [acc("a", 30.0, 0.0, None, None), acc("b", 0.0, 0.0, None, None), acc("c", 0.0, 0.0, None, None)];
        assert_eq!(decide(&input(&a, "a", Strategy::RoundRobin)), None);
        let a = [acc("a", 41.0, 0.0, None, None), acc("b", 0.0, 0.0, None, None), acc("c", 0.0, 0.0, None, None)];
        let d = decide(&input(&a, "a", Strategy::RoundRobin)).unwrap();
        assert_eq!(d.target, "b");
        assert_eq!(d.new_lap, 1);
    }

    #[test]
    fn round_robin_wraps_and_starts_next_lap() {
        let a = [acc("a", 41.0, 0.0, None, None), acc("b", 0.0, 0.0, None, None), acc("c", 41.0, 0.0, None, None)];
        let d = decide(&input(&a, "c", Strategy::RoundRobin)).unwrap();
        // Wrapping to "a" starts lap 2 whose ceiling (80%) it is still under.
        assert_eq!(d.target, "a");
        assert_eq!(d.new_lap, 2);
    }

    #[test]
    fn round_robin_skips_accounts_past_hard_threshold() {
        let a = [acc("a", 41.0, 0.0, None, None), acc("b", 95.0, 0.0, None, None), acc("c", 0.0, 0.0, None, None)];
        assert_eq!(decide(&input(&a, "a", Strategy::RoundRobin)).unwrap().target, "c");
    }

    #[test]
    fn unusable_active_forces_a_switch() {
        let mut a = vec![acc("a", 0.0, 0.0, None, None), acc("b", 10.0, 10.0, None, None)];
        a[0].usable = false;
        assert_eq!(decide(&input(&a, "a", Strategy::Headroom)).unwrap().target, "b");
    }

    #[test]
    fn nothing_to_switch_to_returns_none() {
        let a = [acc("a", 95.0, 0.0, None, None), acc("b", 99.0, 0.0, None, None)];
        for s in [Strategy::Headroom, Strategy::Order, Strategy::Balanced, Strategy::RoundRobin] {
            assert_eq!(decide(&input(&a, "a", s)), None, "{s:?}");
        }
    }
}
