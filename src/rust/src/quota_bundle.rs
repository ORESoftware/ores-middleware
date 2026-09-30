use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

pub const MAX_QUOTA_WINDOWS: usize = 16;
pub const MAX_QUOTA_COST: u32 = 1_000_000;
pub const MAX_QUOTA_LIMIT: u32 = 2_147_483_647;
pub const MAX_QUOTA_WINDOW_MS: u64 = 31 * 24 * 60 * 60 * 1_000;
pub const MAX_QUOTA_REPLAY_RECEIPTS: usize = 2_048;

const MAX_QUOTA_IDENTIFIER_BYTES: usize = 128;
const MIN_ADMISSION_ID_BYTES: usize = 8;
const MAX_ADMISSION_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaBundleError {
    pub code: &'static str,
    pub message: String,
}

impl QuotaBundleError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for QuotaBundleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for QuotaBundleError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuotaEpoch(u64);

impl QuotaEpoch {
    pub fn new(value: u64) -> Result<Self, QuotaBundleError> {
        if value == 0 {
            return Err(QuotaBundleError::new(
                "quota_epoch_invalid",
                "quota epoch must be greater than zero",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Serialize for QuotaEpoch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for QuotaEpoch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_quota_epoch(&value).map_err(de::Error::custom)
    }
}

fn parse_quota_epoch(value: &str) -> Result<QuotaEpoch, QuotaBundleError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(QuotaBundleError::new(
            "quota_epoch_invalid",
            "quota epoch must be a canonical positive decimal string",
        ));
    }
    let parsed = value.parse::<u64>().map_err(|_| {
        QuotaBundleError::new(
            "quota_epoch_invalid",
            "quota epoch exceeds the supported unsigned 64-bit range",
        )
    })?;
    QuotaEpoch::new(parsed)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaWindowPolicy {
    pub policy_id: String,
    pub limit: u32,
    pub window_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaBundlePolicy {
    pub bundle_id: String,
    pub quota_epoch: QuotaEpoch,
    pub windows: Vec<QuotaWindowPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaAdmissionRequest {
    pub principal_digest: String,
    pub operation_digest: String,
    pub admission_id: String,
    pub quota_epoch: QuotaEpoch,
    pub cost: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QuotaAdmissionKind {
    Allowed,
    ReplayedAllowed,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaWindowDecision {
    pub policy_id: String,
    pub limit: u32,
    pub remaining: u32,
    pub reset_after_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaAdmissionDecision {
    pub kind: QuotaAdmissionKind,
    pub bundle_id: String,
    pub quota_epoch: QuotaEpoch,
    pub cost: u32,
    pub windows: Vec<QuotaWindowDecision>,
    pub exhausted_policy_id: Option<String>,
    pub retry_after_ms: Option<u64>,
    pub reason_code: Option<String>,
}

impl QuotaAdmissionDecision {
    pub const fn is_allowed(&self) -> bool {
        matches!(
            self.kind,
            QuotaAdmissionKind::Allowed | QuotaAdmissionKind::ReplayedAllowed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowState {
    window_started_at_ms: u64,
    used: u32,
}

#[derive(Debug, Clone)]
struct ReplayReceipt {
    quota_epoch: QuotaEpoch,
    operation_digest: String,
    cost: u32,
    expires_at_ms: u64,
    decision: QuotaAdmissionDecision,
}

#[derive(Debug, Clone, Default)]
pub struct QuotaBundleState {
    principal_digest: Option<String>,
    bundle_id: Option<String>,
    policy_fingerprint: Option<String>,
    windows: BTreeMap<String, WindowState>,
    replay: HashMap<String, ReplayReceipt>,
    replay_expiry: BinaryHeap<Reverse<(u64, String)>>,
    last_now_ms: Option<u64>,
}

pub fn evaluate_quota_bundle(
    policy: &QuotaBundlePolicy,
    request: &QuotaAdmissionRequest,
    now_ms: u64,
    state: &mut QuotaBundleState,
) -> Result<QuotaAdmissionDecision, QuotaBundleError> {
    validate_policy(policy)?;
    validate_request(request)?;
    validate_clock(now_ms, state.last_now_ms)?;
    validate_state_scope(policy, request, state)?;

    purge_expired_receipts(now_ms, state);

    if let Some(receipt) = state.replay.get(&request.admission_id) {
        state.last_now_ms = Some(now_ms);
        if receipt.quota_epoch == request.quota_epoch
            && receipt.operation_digest == request.operation_digest
            && receipt.cost == request.cost
        {
            let mut decision = receipt.decision.clone();
            decision.kind = QuotaAdmissionKind::ReplayedAllowed;
            return Ok(decision);
        }

        return Ok(QuotaAdmissionDecision {
            kind: QuotaAdmissionKind::Denied,
            bundle_id: policy.bundle_id.clone(),
            quota_epoch: policy.quota_epoch,
            cost: request.cost,
            windows: Vec::new(),
            exhausted_policy_id: None,
            retry_after_ms: None,
            reason_code: Some("admission_replay_mismatch".into()),
        });
    }

    if request.quota_epoch != policy.quota_epoch {
        state.last_now_ms = Some(now_ms);
        let reason = if request.quota_epoch < policy.quota_epoch {
            "stale_quota_epoch"
        } else {
            "future_quota_epoch"
        };
        return Ok(QuotaAdmissionDecision {
            kind: QuotaAdmissionKind::Denied,
            bundle_id: policy.bundle_id.clone(),
            quota_epoch: policy.quota_epoch,
            cost: request.cost,
            windows: Vec::new(),
            exhausted_policy_id: None,
            retry_after_ms: None,
            reason_code: Some(reason.into()),
        });
    }

    let mut current = Vec::with_capacity(policy.windows.len());
    let mut exhausted: Option<(String, u64)> = None;

    if let Some(window) = policy
        .windows
        .iter()
        .find(|window| request.cost > window.limit)
    {
        state.last_now_ms = Some(now_ms);
        return Ok(QuotaAdmissionDecision {
            kind: QuotaAdmissionKind::Denied,
            bundle_id: policy.bundle_id.clone(),
            quota_epoch: policy.quota_epoch,
            cost: request.cost,
            windows: Vec::new(),
            exhausted_policy_id: Some(window.policy_id.clone()),
            retry_after_ms: None,
            reason_code: Some("quota_cost_exceeds_policy_limit".into()),
        });
    }

    for window in &policy.windows {
        let window_started_at_ms = aligned_window_start(now_ms, window.window_ms);
        let used = state
            .windows
            .get(&window.policy_id)
            .filter(|value| value.window_started_at_ms == window_started_at_ms)
            .map_or(0, |value| value.used);
        let remaining = window.limit.saturating_sub(used);
        let reset_after_ms = window
            .window_ms
            .saturating_sub(now_ms.saturating_sub(window_started_at_ms))
            .max(1);

        if request.cost > remaining && exhausted.is_none() {
            exhausted = Some((window.policy_id.clone(), reset_after_ms));
        }

        current.push((
            window,
            WindowState {
                window_started_at_ms,
                used,
            },
            remaining,
            reset_after_ms,
        ));
    }

    if let Some((policy_id, retry_after_ms)) = exhausted {
        state.last_now_ms = Some(now_ms);
        return Ok(QuotaAdmissionDecision {
            kind: QuotaAdmissionKind::Denied,
            bundle_id: policy.bundle_id.clone(),
            quota_epoch: policy.quota_epoch,
            cost: request.cost,
            windows: current
                .into_iter()
                .map(|(window, _, remaining, reset_after_ms)| QuotaWindowDecision {
                    policy_id: window.policy_id.clone(),
                    limit: window.limit,
                    remaining,
                    reset_after_ms,
                })
                .collect(),
            exhausted_policy_id: Some(policy_id),
            retry_after_ms: Some(retry_after_ms),
            reason_code: Some("quota_exhausted".into()),
        });
    }

    if state.replay.len() >= MAX_QUOTA_REPLAY_RECEIPTS {
        return Err(QuotaBundleError::new(
            "quota_replay_capacity_exceeded",
            "quota replay receipts are full; refusing an untracked debit",
        ));
    }

    let replay_ttl_ms = current
        .iter()
        .map(|(_, _, _, reset_after_ms)| *reset_after_ms)
        .max()
        .ok_or_else(|| QuotaBundleError::new("quota_windows_empty", "quota bundle is empty"))?;
    let expires_at_ms = now_ms.checked_add(replay_ttl_ms).ok_or_else(|| {
        QuotaBundleError::new(
            "quota_time_overflow",
            "quota replay receipt expiration overflowed",
        )
    })?;

    let mut next_windows = BTreeMap::new();
    let window_decisions = current
        .into_iter()
        .map(|(window, mut window_state, _, reset_after_ms)| {
            window_state.used = window_state.used.saturating_add(request.cost);
            let remaining = window.limit.saturating_sub(window_state.used);
            next_windows.insert(window.policy_id.clone(), window_state);
            QuotaWindowDecision {
                policy_id: window.policy_id.clone(),
                limit: window.limit,
                remaining,
                reset_after_ms,
            }
        })
        .collect::<Vec<_>>();

    let decision = QuotaAdmissionDecision {
        kind: QuotaAdmissionKind::Allowed,
        bundle_id: policy.bundle_id.clone(),
        quota_epoch: policy.quota_epoch,
        cost: request.cost,
        windows: window_decisions,
        exhausted_policy_id: None,
        retry_after_ms: None,
        reason_code: None,
    };

    state.windows = next_windows;
    state.replay.insert(
        request.admission_id.clone(),
        ReplayReceipt {
            quota_epoch: request.quota_epoch,
            operation_digest: request.operation_digest.clone(),
            cost: request.cost,
            expires_at_ms,
            decision: decision.clone(),
        },
    );
    state
        .replay_expiry
        .push(Reverse((expires_at_ms, request.admission_id.clone())));
    state.last_now_ms = Some(now_ms);

    Ok(decision)
}

fn validate_policy(policy: &QuotaBundlePolicy) -> Result<(), QuotaBundleError> {
    if !valid_identifier(&policy.bundle_id) {
        return Err(QuotaBundleError::new(
            "quota_bundle_id_invalid",
            "quota bundle IDs must be 1..=128 ASCII letters, digits, dot, underscore, or hyphen",
        ));
    }
    if policy.windows.is_empty() || policy.windows.len() > MAX_QUOTA_WINDOWS {
        return Err(QuotaBundleError::new(
            "quota_window_count_invalid",
            format!(
                "quota bundles require 1..={MAX_QUOTA_WINDOWS} windows"
            ),
        ));
    }

    let mut seen = BTreeMap::<&str, ()>::new();
    for window in &policy.windows {
        if !valid_identifier(&window.policy_id) {
            return Err(QuotaBundleError::new(
                "quota_policy_id_invalid",
                "quota policy IDs must be 1..=128 ASCII letters, digits, dot, underscore, or hyphen",
            ));
        }
        if seen.insert(&window.policy_id, ()).is_some() {
            return Err(QuotaBundleError::new(
                "quota_policy_id_duplicate",
                "quota policy IDs must be unique within a bundle",
            ));
        }
        if window.limit == 0 || window.limit > MAX_QUOTA_LIMIT {
            return Err(QuotaBundleError::new(
                "quota_limit_invalid",
                format!("quota limits must be 1..={MAX_QUOTA_LIMIT}"),
            ));
        }
        if window.window_ms == 0 || window.window_ms > MAX_QUOTA_WINDOW_MS {
            return Err(QuotaBundleError::new(
                "quota_window_invalid",
                format!("quota windows must be 1..={MAX_QUOTA_WINDOW_MS} ms"),
            ));
        }
    }
    Ok(())
}

fn validate_request(request: &QuotaAdmissionRequest) -> Result<(), QuotaBundleError> {
    if !valid_digest(&request.principal_digest) {
        return Err(QuotaBundleError::new(
            "quota_principal_digest_invalid",
            "quota principal digest must be 64 lowercase hexadecimal characters",
        ));
    }
    if !valid_digest(&request.operation_digest) {
        return Err(QuotaBundleError::new(
            "quota_operation_digest_invalid",
            "quota operation digest must be 64 lowercase hexadecimal characters",
        ));
    }
    if !valid_admission_id(&request.admission_id) {
        return Err(QuotaBundleError::new(
            "quota_admission_id_invalid",
            "quota admission IDs must be bounded ASCII tokens",
        ));
    }
    if request.cost == 0 || request.cost > MAX_QUOTA_COST {
        return Err(QuotaBundleError::new(
            "quota_cost_invalid",
            format!("quota cost must be 1..={MAX_QUOTA_COST}"),
        ));
    }
    Ok(())
}

fn validate_clock(now_ms: u64, previous: Option<u64>) -> Result<(), QuotaBundleError> {
    if previous.is_some_and(|previous| now_ms < previous) {
        return Err(QuotaBundleError::new(
            "quota_clock_regression",
            "quota coordinator clock moved backwards",
        ));
    }
    Ok(())
}

fn validate_state_scope(
    policy: &QuotaBundlePolicy,
    request: &QuotaAdmissionRequest,
    state: &mut QuotaBundleState,
) -> Result<(), QuotaBundleError> {
    if state
        .principal_digest
        .as_deref()
        .is_some_and(|value| value != request.principal_digest)
    {
        return Err(QuotaBundleError::new(
            "quota_state_principal_mismatch",
            "quota state was reused across principals",
        ));
    }
    if state
        .bundle_id
        .as_deref()
        .is_some_and(|value| value != policy.bundle_id)
    {
        return Err(QuotaBundleError::new(
            "quota_state_bundle_mismatch",
            "quota state was reused across bundles",
        ));
    }

    let fingerprint = policy_fingerprint(policy);
    if state
        .policy_fingerprint
        .as_deref()
        .is_some_and(|value| value != fingerprint)
    {
        return Err(QuotaBundleError::new(
            "quota_policy_changed_without_state_migration",
            "quota window structure changed for an existing ledger",
        ));
    }

    if state.principal_digest.is_none() {
        state.principal_digest = Some(request.principal_digest.clone());
    }
    if state.bundle_id.is_none() {
        state.bundle_id = Some(policy.bundle_id.clone());
    }
    if state.policy_fingerprint.is_none() {
        state.policy_fingerprint = Some(fingerprint);
    }
    Ok(())
}

fn policy_fingerprint(policy: &QuotaBundlePolicy) -> String {
    policy
        .windows
        .iter()
        .map(|window| {
            format!(
                "{}:{}:{}",
                window.policy_id, window.limit, window.window_ms
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn purge_expired_receipts(now_ms: u64, state: &mut QuotaBundleState) {
    while let Some(Reverse((expires_at_ms, admission_id))) = state.replay_expiry.peek() {
        if *expires_at_ms > now_ms {
            break;
        }
        let expires_at_ms = *expires_at_ms;
        let admission_id = admission_id.clone();
        state.replay_expiry.pop();
        if state
            .replay
            .get(&admission_id)
            .is_some_and(|receipt| receipt.expires_at_ms == expires_at_ms)
        {
            state.replay.remove(&admission_id);
        }
    }
}

const fn aligned_window_start(now_ms: u64, window_ms: u64) -> u64 {
    now_ms - (now_ms % window_ms)
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_QUOTA_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_admission_id(value: &str) -> bool {
    value.len() >= MIN_ADMISSION_ID_BYTES
        && value.len() <= MAX_ADMISSION_ID_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch(value: u64) -> QuotaEpoch {
        QuotaEpoch::new(value).expect("valid epoch")
    }

    fn policy(epoch_value: u64) -> QuotaBundlePolicy {
        QuotaBundlePolicy {
            bundle_id: "tenant-default".into(),
            quota_epoch: epoch(epoch_value),
            windows: vec![
                QuotaWindowPolicy {
                    policy_id: "minute".into(),
                    limit: 5,
                    window_ms: 60_000,
                },
                QuotaWindowPolicy {
                    policy_id: "ten-minute".into(),
                    limit: 7,
                    window_ms: 600_000,
                },
                QuotaWindowPolicy {
                    policy_id: "hour".into(),
                    limit: 10,
                    window_ms: 3_600_000,
                },
            ],
        }
    }

    fn request(id: &str, epoch_value: u64, cost: u32) -> QuotaAdmissionRequest {
        QuotaAdmissionRequest {
            principal_digest: "a".repeat(64),
            operation_digest: "b".repeat(64),
            admission_id: id.into(),
            quota_epoch: epoch(epoch_value),
            cost,
        }
    }

    #[test]
    fn quota_epoch_serializes_as_canonical_decimal_string() {
        let encoded = serde_json::to_string(&epoch(42)).expect("serialize epoch");
        assert_eq!(encoded, "\"42\"");
        assert_eq!(
            serde_json::from_str::<QuotaEpoch>("\"42\"")
                .expect("deserialize epoch")
                .get(),
            42
        );
        assert!(serde_json::from_str::<QuotaEpoch>("42").is_err());
        assert!(serde_json::from_str::<QuotaEpoch>("\"042\"").is_err());
        assert!(serde_json::from_str::<QuotaEpoch>("\"0\"").is_err());
    }

    #[test]
    fn all_windows_debit_atomically_or_not_at_all() {
        let policy = policy(3);
        let mut state = QuotaBundleState::default();

        let first = evaluate_quota_bundle(
            &policy,
            &request("operation-0001", 3, 3),
            10_000,
            &mut state,
        )
        .expect("first admission");
        assert!(first.is_allowed());
        assert_eq!(
            first
                .windows
                .iter()
                .map(|window| window.remaining)
                .collect::<Vec<_>>(),
            vec![2, 4, 7]
        );

        let before = state.windows.clone();
        let denied = evaluate_quota_bundle(
            &policy,
            &request("operation-0002", 3, 3),
            10_001,
            &mut state,
        )
        .expect("quota denial");
        assert_eq!(denied.kind, QuotaAdmissionKind::Denied);
        assert_eq!(denied.exhausted_policy_id.as_deref(), Some("minute"));
        assert_eq!(denied.reason_code.as_deref(), Some("quota_exhausted"));
        assert_eq!(state.windows, before);
    }

    #[test]
    fn stale_and_future_epochs_fail_closed_without_debit() {
        let policy = policy(9);
        let mut state = QuotaBundleState::default();

        for (id, presented, reason) in [
            ("operation-0010", 8, "stale_quota_epoch"),
            ("operation-0011", 10, "future_quota_epoch"),
        ] {
            let decision = evaluate_quota_bundle(
                &policy,
                &request(id, presented, 1),
                20_000,
                &mut state,
            )
            .expect("epoch rejection");
            assert_eq!(decision.kind, QuotaAdmissionKind::Denied);
            assert_eq!(decision.reason_code.as_deref(), Some(reason));
            assert!(state.windows.is_empty());
        }
    }

    #[test]
    fn allowed_admission_replays_without_double_charge() {
        let policy = policy(5);
        let mut state = QuotaBundleState::default();
        let request = request("operation-0020", 5, 2);

        let first =
            evaluate_quota_bundle(&policy, &request, 30_000, &mut state).expect("admit request");
        assert_eq!(first.kind, QuotaAdmissionKind::Allowed);
        let before = state.windows.clone();

        let replay =
            evaluate_quota_bundle(&policy, &request, 30_001, &mut state).expect("replay request");
        assert_eq!(replay.kind, QuotaAdmissionKind::ReplayedAllowed);
        assert_eq!(state.windows, before);
    }

    #[test]
    fn admission_id_cannot_be_reused_for_different_operation_or_cost() {
        let policy = policy(5);
        let mut state = QuotaBundleState::default();
        let first = request("operation-0030", 5, 1);
        evaluate_quota_bundle(&policy, &first, 40_000, &mut state).expect("admit request");
        let before = state.windows.clone();

        let mut different_operation = first.clone();
        different_operation.operation_digest = "c".repeat(64);
        let denied = evaluate_quota_bundle(
            &policy,
            &different_operation,
            40_001,
            &mut state,
        )
        .expect("replay mismatch denial");
        assert_eq!(denied.reason_code.as_deref(), Some("admission_replay_mismatch"));
        assert_eq!(state.windows, before);

        let mut different_cost = first;
        different_cost.cost = 2;
        let denied =
            evaluate_quota_bundle(&policy, &different_cost, 40_002, &mut state)
                .expect("replay mismatch denial");
        assert_eq!(denied.reason_code.as_deref(), Some("admission_replay_mismatch"));
        assert_eq!(state.windows, before);
    }

    #[test]
    fn cost_larger_than_policy_limit_is_not_advertised_as_retryable() {
        let policy = policy(5);
        let mut state = QuotaBundleState::default();
        let decision = evaluate_quota_bundle(
            &policy,
            &request("operation-0035", 5, 6),
            45_000,
            &mut state,
        )
        .expect("policy cost rejection");

        assert_eq!(decision.kind, QuotaAdmissionKind::Denied);
        assert_eq!(
            decision.reason_code.as_deref(),
            Some("quota_cost_exceeds_policy_limit")
        );
        assert_eq!(decision.exhausted_policy_id.as_deref(), Some("minute"));
        assert_eq!(decision.retry_after_ms, None);
        assert!(state.windows.is_empty());
    }

    #[test]
    fn replay_receipt_expires_at_latest_charged_window_boundary() {
        let mut policy = policy(5);
        policy.windows = vec![
            QuotaWindowPolicy {
                policy_id: "minute".into(),
                limit: 5,
                window_ms: 60_000,
            },
            QuotaWindowPolicy {
                policy_id: "ten-minute".into(),
                limit: 10,
                window_ms: 600_000,
            },
        ];
        let mut state = QuotaBundleState::default();
        let request = request("operation-0036", 5, 1);

        evaluate_quota_bundle(&policy, &request, 599_999, &mut state)
            .expect("initial admission");
        let receipt = state
            .replay
            .get("operation-0036")
            .expect("replay receipt");
        assert_eq!(receipt.expires_at_ms, 600_000);

        let replay = evaluate_quota_bundle(&policy, &request, 599_999, &mut state)
            .expect("pre-boundary replay");
        assert_eq!(replay.kind, QuotaAdmissionKind::ReplayedAllowed);

        let fresh = evaluate_quota_bundle(&policy, &request, 600_000, &mut state)
            .expect("post-boundary admission");
        assert_eq!(fresh.kind, QuotaAdmissionKind::Allowed);
    }
    #[test]
    fn fixed_window_boundary_rollover_is_deterministic() {
        let mut policy = policy(2);
        policy.windows = vec![QuotaWindowPolicy {
            policy_id: "minute".into(),
            limit: 2,
            window_ms: 60_000,
        }];
        let mut state = QuotaBundleState::default();

        let first = evaluate_quota_bundle(
            &policy,
            &request("operation-0040", 2, 2),
            59_999,
            &mut state,
        )
        .expect("admit final millisecond");
        assert_eq!(first.windows[0].remaining, 0);
        assert_eq!(first.windows[0].reset_after_ms, 1);

        let denied = evaluate_quota_bundle(
            &policy,
            &request("operation-0041", 2, 1),
            59_999,
            &mut state,
        )
        .expect("deny same window");
        assert_eq!(denied.kind, QuotaAdmissionKind::Denied);

        let next = evaluate_quota_bundle(
            &policy,
            &request("operation-0042", 2, 1),
            60_000,
            &mut state,
        )
        .expect("admit new window");
        assert_eq!(next.kind, QuotaAdmissionKind::Allowed);
        assert_eq!(next.windows[0].remaining, 1);
        assert_eq!(next.windows[0].reset_after_ms, 60_000);
    }

    #[test]
    fn clock_regression_and_cross_principal_state_reuse_fail_closed() {
        let policy = policy(1);
        let mut state = QuotaBundleState::default();
        evaluate_quota_bundle(
            &policy,
            &request("operation-0050", 1, 1),
            50_000,
            &mut state,
        )
        .expect("initial admission");

        let error = evaluate_quota_bundle(
            &policy,
            &request("operation-0051", 1, 1),
            49_999,
            &mut state,
        )
        .expect_err("clock regression");
        assert_eq!(error.code, "quota_clock_regression");

        let mut other = request("operation-0052", 1, 1);
        other.principal_digest = "d".repeat(64);
        let error = evaluate_quota_bundle(&policy, &other, 50_001, &mut state)
            .expect_err("cross-principal state reuse");
        assert_eq!(error.code, "quota_state_principal_mismatch");
    }

    #[test]
    fn policy_structure_cannot_mutate_in_place() {
        let policy = policy(1);
        let mut state = QuotaBundleState::default();
        evaluate_quota_bundle(
            &policy,
            &request("operation-0060", 1, 1),
            60_000,
            &mut state,
        )
        .expect("initial admission");

        let mut mutated = policy.clone();
        mutated.windows[0].limit = 999;
        let error = evaluate_quota_bundle(
            &mutated,
            &request("operation-0061", 1, 1),
            60_001,
            &mut state,
        )
        .expect_err("in-place policy mutation");
        assert_eq!(error.code, "quota_policy_changed_without_state_migration");
    }

    #[test]
    fn malformed_cost_and_identifiers_fail_before_debit() {
        let mut bad_policy = policy(1);
        let mut state = QuotaBundleState::default();
        bad_policy.windows[0].policy_id = "bad policy".into();
        let error = evaluate_quota_bundle(
            &bad_policy,
            &request("operation-0070", 1, 1),
            70_000,
            &mut state,
        )
        .expect_err("invalid policy id");
        assert_eq!(error.code, "quota_policy_id_invalid");
        assert!(state.windows.is_empty());

        let policy = policy(1);
        let error = evaluate_quota_bundle(
            &policy,
            &request("operation-0071", 1, 0),
            70_000,
            &mut state,
        )
        .expect_err("zero cost");
        assert_eq!(error.code, "quota_cost_invalid");
        assert!(state.windows.is_empty());
    }
}
