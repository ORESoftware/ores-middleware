export const DEFAULT_DRAIN_TIMEOUT_MS = 5_000;
export const DEFAULT_RETRY_AFTER_MS = 5_000;
export const MAX_TIMER_DELAY_MS = 2_147_483_647;
export const SHUTDOWN_HTTP_STATUS = 429 as const;

export type ShutdownPhase = "running" | "draining" | "forced";

export interface ShutdownRejection {
  readonly status: typeof SHUTDOWN_HTTP_STATUS;
  readonly code: "service_draining";
  readonly message: string;
  /** End-to-end metadata only; hop-by-hop fields belong to transport adapters. */
  readonly headers: Readonly<Record<string, string>>;
}

export type DrainOutcome =
  | { readonly kind: "drained"; readonly active_at_start: number }
  | { readonly kind: "timed_out"; readonly remaining: number }
  | { readonly kind: "forced"; readonly remaining: number };

export interface DrainLease {
  finish(): void;
}

export type RequestAdmission =
  | { readonly ok: true; readonly lease: DrainLease }
  | { readonly ok: false; readonly rejection: ShutdownRejection };

/**
 * Framework-neutral request drain coordinator.
 *
 * This module deliberately does not install process signal handlers or inspect
 * stdin/TTY state. Consumers own the actual shutdown trigger, listener
 * lifecycle, middleware ordering, telemetry flush, and final process exit.
 */
export class ShutdownCoordinator {
  #phase: ShutdownPhase = "running";
  #activeRequests = 0;
  readonly #drainTimeoutMs: number;
  readonly #retryAfterMs: number;
  readonly #waiters = new Set<() => void>();

  constructor(
    drainTimeoutMs = DEFAULT_DRAIN_TIMEOUT_MS,
    retryAfterMs = DEFAULT_RETRY_AFTER_MS
  ) {
    assertDuration("drainTimeoutMs", drainTimeoutMs, true);
    assertDuration("retryAfterMs", retryAfterMs, false);
    this.#drainTimeoutMs = drainTimeoutMs;
    this.#retryAfterMs = retryAfterMs;
  }

  get phase(): ShutdownPhase {
    return this.#phase;
  }

  get activeRequests(): number {
    return this.#activeRequests;
  }

  get isAcceptingRequests(): boolean {
    return this.#phase === "running";
  }

  get isForced(): boolean {
    return this.#phase === "forced";
  }

  tryBeginRequest(): RequestAdmission {
    if (this.#phase !== "running") {
      return { ok: false, rejection: this.rejection() };
    }

    this.#activeRequests += 1;
    let finished = false;
    return {
      ok: true,
      lease: {
        finish: () => {
          if (finished) return;
          finished = true;
          this.#activeRequests -= 1;
          if (this.#activeRequests < 0) {
            this.#activeRequests = 0;
            throw new Error("shutdown request accounting underflow");
          }
          this.#notify();
        }
      }
    };
  }

  startDraining(): boolean {
    if (this.#phase !== "running") return false;
    this.#phase = "draining";
    this.#notify();
    return true;
  }

  forceShutdown(): boolean {
    if (this.#phase === "forced") return false;
    this.#phase = "forced";
    this.#notify();
    return true;
  }

  async drain(): Promise<DrainOutcome> {
    this.startDraining();
    const activeAtStart = this.#activeRequests;

    // Observe forced state through a boolean accessor at async control-flow
    // boundaries. TypeScript otherwise narrows the literal phase from the
    // earlier check and can incorrectly treat a later forced transition as
    // unreachable even though forceShutdown() may run while this method waits.
    if (this.isForced) {
      return { kind: "forced", remaining: activeAtStart };
    }
    if (activeAtStart === 0) {
      return { kind: "drained", active_at_start: 0 };
    }

    const deadline = performance.now() + this.#drainTimeoutMs;
    while (this.#activeRequests > 0 && !this.isForced) {
      const remainingMs = Math.max(0, deadline - performance.now());
      if (remainingMs === 0) {
        const remaining = this.#activeRequests;
        this.forceShutdown();
        return { kind: "timed_out", remaining };
      }

      const event = await this.#waitForChangeOrTimeout(remainingMs);
      if (event === "timeout" && this.#activeRequests > 0) {
        const remaining = this.#activeRequests;
        this.forceShutdown();
        return { kind: "timed_out", remaining };
      }
    }

    if (this.#activeRequests === 0) {
      return { kind: "drained", active_at_start: activeAtStart };
    }
    return { kind: "forced", remaining: this.#activeRequests };
  }

  rejection(): ShutdownRejection {
    const retryAfterSeconds = Math.max(1, Math.ceil(this.#retryAfterMs / 1_000));
    return {
      status: SHUTDOWN_HTTP_STATUS,
      code: "service_draining",
      message: "service is draining and is not accepting new requests",
      headers: Object.freeze({
        "retry-after": String(retryAfterSeconds)
      })
    };
  }

  #notify(): void {
    const waiters = [...this.#waiters];
    this.#waiters.clear();
    for (const waiter of waiters) waiter();
  }

  #waitForChangeOrTimeout(timeoutMs: number): Promise<"changed" | "timeout"> {
    return new Promise((resolve) => {
      let settled = false;
      let timer: ReturnType<typeof setTimeout> | undefined;
      const onChange = () => {
        if (settled) return;
        settled = true;
        if (timer !== undefined) clearTimeout(timer);
        this.#waiters.delete(onChange);
        resolve("changed");
      };

      this.#waiters.add(onChange);
      // Re-check after registering so a request finishing immediately before
      // waiter registration cannot leave drain() asleep until the timeout.
      if (this.#activeRequests === 0 || this.#phase === "forced") {
        onChange();
        return;
      }

      timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        this.#waiters.delete(onChange);
        resolve("timeout");
      }, timeoutMs);
    });
  }
}

function assertDuration(name: string, value: number, allowZero: boolean): void {
  if (
    !Number.isFinite(value) ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > MAX_TIMER_DELAY_MS ||
    (!allowZero && value === 0)
  ) {
    throw new RangeError(
      `${name} must be ${allowZero ? "a non-negative" : "a positive"} integer number of milliseconds no greater than ${MAX_TIMER_DELAY_MS}`
    );
  }
}
