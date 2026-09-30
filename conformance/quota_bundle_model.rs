//! Bounded model checker for atomic quota-bundle admission.
//! The model intentionally uses tiny bounds so every state can be exhausted
//! quickly in CI while proving the all-or-none debit and replay invariants.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Outcome {
    Allowed,
    ReplayedAllowed,
    Denied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct State {
    minute_used: u8,
    hour_used: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Transition {
    outcome: Outcome,
    before: State,
    after: State,
}

const MINUTE_LIMIT: u8 = 2;
const HOUR_LIMIT: u8 = 3;

fn transition(
    before: State,
    cost: u8,
    epoch_matches: bool,
    replay_exists: bool,
    replay_matches: bool,
) -> Transition {
    if replay_exists {
        return Transition {
            outcome: if replay_matches {
                Outcome::ReplayedAllowed
            } else {
                Outcome::Denied
            },
            before,
            after: before,
        };
    }

    if !epoch_matches
        || cost == 0
        || cost > MINUTE_LIMIT
        || cost > HOUR_LIMIT
        || before.minute_used.saturating_add(cost) > MINUTE_LIMIT
        || before.hour_used.saturating_add(cost) > HOUR_LIMIT
    {
        return Transition {
            outcome: Outcome::Denied,
            before,
            after: before,
        };
    }

    Transition {
        outcome: Outcome::Allowed,
        before,
        after: State {
            minute_used: before.minute_used + cost,
            hour_used: before.hour_used + cost,
        },
    }
}

fn invariant(transition: Transition, cost: u8) -> bool {
    match transition.outcome {
        Outcome::Allowed => {
            transition.after.minute_used == transition.before.minute_used + cost
                && transition.after.hour_used == transition.before.hour_used + cost
        }
        Outcome::ReplayedAllowed | Outcome::Denied => transition.after == transition.before,
    }
}

fn main() {
    let mut explored = 0usize;

    for minute_used in 0..=MINUTE_LIMIT {
        for hour_used in 0..=HOUR_LIMIT {
            for cost in 0..=HOUR_LIMIT + 1 {
                for epoch_matches in [false, true] {
                    for replay_exists in [false, true] {
                        for replay_matches in [false, true] {
                            let before = State {
                                minute_used,
                                hour_used,
                            };
                            let candidate = transition(
                                before,
                                cost,
                                epoch_matches,
                                replay_exists,
                                replay_matches,
                            );
                            assert!(
                                invariant(candidate, cost),
                                "atomic debit invariant failed: {candidate:?}, cost={cost}"
                            );

                            if candidate.outcome == Outcome::Allowed {
                                assert!(epoch_matches);
                                assert!(!replay_exists);
                                assert!(cost > 0);
                                assert!(candidate.after.minute_used <= MINUTE_LIMIT);
                                assert!(candidate.after.hour_used <= HOUR_LIMIT);
                            }

                            if replay_exists {
                                assert_eq!(
                                    candidate.after, before,
                                    "replay must never mutate quota counters"
                                );
                                assert_eq!(
                                    candidate.outcome == Outcome::ReplayedAllowed,
                                    replay_matches,
                                    "only an exact receipt match may replay an allow"
                                );
                            }

                            if !epoch_matches && !replay_exists {
                                assert_eq!(
                                    candidate.outcome,
                                    Outcome::Denied,
                                    "fresh stale/future epoch must fail closed"
                                );
                            }

                            let minute_exhausted =
                                minute_used.saturating_add(cost) > MINUTE_LIMIT;
                            let hour_exhausted = hour_used.saturating_add(cost) > HOUR_LIMIT;
                            if !replay_exists
                                && (minute_exhausted
                                    || hour_exhausted
                                    || cost == 0
                                    || cost > MINUTE_LIMIT
                                    || cost > HOUR_LIMIT)
                            {
                                assert_eq!(candidate.outcome, Outcome::Denied);
                                assert_eq!(
                                    candidate.after, before,
                                    "failed admission must debit no window"
                                );
                            }

                            explored += 1;
                        }
                    }
                }
            }
        }
    }

    println!(
        "ores-middleware quota bundle model: explored {explored} transitions; invariants hold"
    );
}
