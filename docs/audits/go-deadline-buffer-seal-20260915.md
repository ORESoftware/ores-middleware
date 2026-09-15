# Go deadline buffer sealing audit — 2026-09-15

Tracks #176.

## Failure

The Windows CI failure in `TestDeadlineSealsHandlerBufferAgainstLateWrites` returned a nil error from a handler write that occurred after the request deadline. The final client response still became `504`, so the problem was not response selection; it was the handler-visible write contract.

## Root cause

The previous implementation sealed `bufferedResponse` only after the middleware goroutine woke on `ctx.Done()`. If the request deadline became ready while both the handler and middleware goroutines were runnable, the handler could acquire the response mutex and write before the middleware goroutine called `seal()`. This is scheduler-order dependent and was reproducible on Windows.

## Fix

The handler-owned response buffer now receives the request `ctx.Done()` channel. Under the same mutex used by `Write` and `WriteHeader`, it checks whether that channel has closed; if so it transitions to sealed before admitting any mutation. `Write` returns `http.ErrHandlerTimeout` and `WriteHeader` becomes a no-op.

The explicit middleware-side `seal()` remains in place as a second fence when the timeout/cancellation branch wins.

## Regression evidence

- A direct deterministic test closes the done signal before a write and proves the buffer rejects both body and status mutation.
- The integration test no longer sleeps for scheduler timing. The handler waits on its actual request context cancellation/deadline and immediately attempts a write, proving the response buffer itself enforces the deadline boundary.
- No Windows-specific skip or timing allowance is introduced.

## Promotion gate

Require exact-head, stepful Go CI on all configured operating systems, including Windows. A runnerless/zero-step result is infrastructure non-evidence and does not satisfy this gate.
