//! Exhaust bounded capability revocation and stale middleware request tokens.
//! Model safety only; not proof of the production DI/async implementation.
use std::collections::{HashSet, VecDeque};

const DB: u8 = 0b001;
const HTTP: u8 = 0b010;
const FS: u8 = 0b100;
const APPROVED: u8 = DB | HTTP;
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct State {
    generation: u8,
    grants: u8,
    ready: bool,
    tokens: [Option<u8>; 2],
}
fn allowed(s: State, index: usize) -> bool {
    let bit = if index == 0 { DB } else { HTTP };
    s.ready && (s.grants & bit != 0) && s.tokens[index] == Some(s.generation)
}
fn next(s: State) -> Vec<State> {
    let mut out = Vec::new();
    for (index, bit) in [(0, DB), (1, HTTP)] {
        if !s.ready && s.grants & bit == 0 {
            out.push(State { grants: s.grants | bit, ..s });
        }
        if s.ready && s.tokens[index] != Some(s.generation) {
            let mut tokens = s.tokens;
            tokens[index] = Some(s.generation);
            out.push(State { tokens, ..s });
        }
    }
    if !s.ready && s.grants == APPROVED {
        out.push(State { ready: true, ..s });
    }
    if s.generation < 2 && (s.ready || s.grants != 0) {
        out.push(State { generation: s.generation + 1, grants: 0, ready: false, ..s });
    }
    out
}
fn main() {
    let initial = State { generation: 0, grants: 0, ready: false, tokens: [None, None] };
    let mut seen = HashSet::from([initial]);
    let mut queue = VecDeque::from([initial]);
    let mut stale_token_witness = false;
    let mut fresh_token_witness = false;
    while let Some(s) = queue.pop_front() {
        assert_eq!(s.grants & FS, 0, "ambient filesystem permission admitted");
        assert!(!s.ready || s.grants == APPROVED, "ready without exact authority");
        for index in 0..2 {
            let valid = s.ready && s.tokens[index] == Some(s.generation);
            assert_eq!(allowed(s, index), valid);
            if s.tokens[index].is_some() && s.tokens[index] != Some(s.generation) {
                assert!(!allowed(s, index), "stale token crossed revoked generation");
                if s.ready { stale_token_witness = true; }
            }
            if allowed(s, index) { fresh_token_witness = true; }
        }
        for candidate in next(s) {
            assert!(candidate.generation == s.generation
                || candidate.generation == s.generation + 1);
            if candidate.generation > s.generation {
                assert!(!candidate.ready && candidate.grants == 0,
                    "revocation must fail closed");
            }
            assert_eq!(candidate.grants & FS, 0);
            if seen.insert(candidate) { queue.push_back(candidate); }
        }
    }
    assert!(stale_token_witness, "did not explore stale tokens after reacquisition");
    assert!(fresh_token_witness, "did not explore valid admitted work");
    assert!(seen.len() > 10, "vacuous authority state space");
    println!("ores-middleware revocation model: {} states explored", seen.len());
}
