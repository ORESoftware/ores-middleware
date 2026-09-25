//! Bounded model checker for dependency-injected middleware authority.
//! A callback can receive only explicitly approved capabilities, and readiness
//! requires the exact approved set rather than ambient/imported authority.

use std::collections::{HashSet, VecDeque};

const DB: u8 = 0b001;
const HTTP: u8 = 0b010;
const FS: u8 = 0b100;
const APPROVED: u8 = DB | HTTP;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct State { injected: u8, ready: bool }

fn invariant(s: State) -> bool {
    let no_ambient_authority = s.injected & !APPROVED == 0;
    let exact_when_ready = !s.ready || s.injected == APPROVED;
    no_ambient_authority && exact_when_ready
}

fn next(s: State) -> Vec<State> {
    let mut out = Vec::new();
    for capability in [DB, HTTP] {
        if s.injected & capability == 0 {
            out.push(State { injected: s.injected | capability, ..s });
        }
    }
    if s.injected == APPROVED && !s.ready {
        out.push(State { ready: true, ..s });
    }
    out
}

fn main() {
    let initial = State { injected: 0, ready: false };
    let mut seen = HashSet::from([initial]);
    let mut queue = VecDeque::from([initial]);
    while let Some(state) = queue.pop_front() {
        assert!(invariant(state), "invalid middleware authority state: {state:?}");
        for candidate in next(state) {
            assert!(invariant(candidate), "authority escaped: {state:?} -> {candidate:?}");
            assert_eq!(candidate.injected & FS, 0, "filesystem authority must remain ambiently unavailable");
            if seen.insert(candidate) { queue.push_back(candidate); }
        }
    }
    assert!(seen.contains(&State { injected: APPROVED, ready: true }));
    println!("ores-middleware formal model: explored {} states; invariants hold", seen.len());
}
