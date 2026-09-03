----------------------- MODULE RateLimitPipeline -----------------------
EXTENDS Naturals

VARIABLES
    phase,
    operation,
    coordinatorUp,
    enforcement,
    consistency,
    failureMode,
    proxyValidated,
    authenticated,
    principalEvaluated,
    authorized,
    handled,
    decision

vars == <<
    phase,
    operation,
    coordinatorUp,
    enforcement,
    consistency,
    failureMode,
    proxyValidated,
    authenticated,
    principalEvaluated,
    authorized,
    handled,
    decision
>>

Operations == {
    "health-read",
    "public-read",
    "auth-attempt",
    "auth-recovery",
    "mutation",
    "payment-or-ledger-write",
    "job-admission"
}

StrictOperations == {
    "auth-attempt",
    "auth-recovery",
    "mutation",
    "payment-or-ledger-write",
    "job-admission"
}

RequiredConsistency(op) ==
    IF op \in StrictOperations
    THEN "strict"
    ELSE IF op = "public-read" THEN "bounded" ELSE "advisory"

RequiredFailureMode(op) ==
    IF op \in StrictOperations
    THEN "fail-closed"
    ELSE IF op = "public-read" THEN "local-only" ELSE "fail-open"

TypeOK ==
    /\ phase \in 0..8
    /\ operation \in Operations
    /\ coordinatorUp \in BOOLEAN
    /\ enforcement \in {"disabled", "audit", "enforce"}
    /\ consistency \in {"strict", "bounded", "advisory"}
    /\ failureMode \in {"fail-open", "fail-closed", "local-only"}
    /\ proxyValidated \in BOOLEAN
    /\ authenticated \in BOOLEAN
    /\ principalEvaluated \in BOOLEAN
    /\ authorized \in BOOLEAN
    /\ handled \in BOOLEAN
    /\ decision \in {"pending", "bypass", "observe", "allow", "deny"}

Init ==
    /\ phase = 0
    /\ operation \in Operations
    /\ coordinatorUp \in BOOLEAN
    /\ enforcement \in {"disabled", "audit", "enforce"}
    /\ consistency = RequiredConsistency(operation)
    /\ failureMode = RequiredFailureMode(operation)
    /\ proxyValidated = FALSE
    /\ authenticated = FALSE
    /\ principalEvaluated = FALSE
    /\ authorized = FALSE
    /\ handled = FALSE
    /\ decision = "pending"

EstablishContext ==
    /\ phase = 0
    /\ phase' = 1
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        proxyValidated, authenticated, principalEvaluated, authorized,
        handled, decision
       >>

ValidateProxy ==
    /\ phase = 1
    /\ phase' = 2
    /\ proxyValidated' = TRUE
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        authenticated, principalEvaluated, authorized, handled, decision
       >>

AnonymousGuard ==
    /\ phase = 2
    /\ phase' = 3
    /\ decision' = IF enforcement = "audit" THEN "observe" ELSE decision
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        proxyValidated, authenticated, principalEvaluated, authorized, handled
       >>

Authenticate ==
    /\ phase = 3
    /\ phase' = 4
    /\ authenticated' = TRUE
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        proxyValidated, principalEvaluated, authorized, handled, decision
       >>

PrincipalLimit ==
    /\ phase = 4
    /\ principalEvaluated' = TRUE
    /\ IF enforcement = "disabled"
       THEN /\ decision' = "bypass"
            /\ phase' = 5
       ELSE IF consistency = "strict" /\ ~coordinatorUp
            THEN IF enforcement = "enforce"
                 THEN /\ decision' = "deny"
                      /\ phase' = 8
                 ELSE /\ decision' = "observe"
                      /\ phase' = 5
            ELSE /\ decision' = IF enforcement = "audit" THEN "observe" ELSE "allow"
                 /\ phase' = 5
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        proxyValidated, authenticated, authorized, handled
       >>

Authorize ==
    /\ phase = 5
    /\ decision # "deny"
    /\ phase' = 6
    /\ authorized' = TRUE
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        proxyValidated, authenticated, principalEvaluated, handled, decision
       >>

Handle ==
    /\ phase = 6
    /\ phase' = 7
    /\ handled' = TRUE
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        proxyValidated, authenticated, principalEvaluated, authorized, decision
       >>

Finalize ==
    /\ phase = 7
    /\ phase' = 8
    /\ UNCHANGED <<
        operation, coordinatorUp, enforcement, consistency, failureMode,
        proxyValidated, authenticated, principalEvaluated, authorized,
        handled, decision
       >>

Next ==
    \/ EstablishContext
    \/ ValidateProxy
    \/ AnonymousGuard
    \/ Authenticate
    \/ PrincipalLimit
    \/ Authorize
    \/ Handle
    \/ Finalize

Spec == Init /\ [][Next]_vars

PostureSound ==
    /\ consistency = RequiredConsistency(operation)
    /\ failureMode = RequiredFailureMode(operation)

ProxyBeforeIdentity ==
    (authenticated \/ principalEvaluated) => proxyValidated

AuthenticationBeforePrincipalLimit ==
    principalEvaluated => authenticated

AuthorizationBeforeHandler ==
    handled => authorized

StrictCoordinatorOutageFailsClosed ==
    operation \in StrictOperations
    /\ enforcement = "enforce"
    /\ ~coordinatorUp
        => ~handled

AuditModeNeverDenies ==
    enforcement = "audit" => decision # "deny"

DeniedRequestsNeverReachHandler ==
    decision = "deny" => ~handled

=======================================================================
