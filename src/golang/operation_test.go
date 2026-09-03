package oresmiddleware

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"
)

func testOperationContext(slot int) RequestContext {
	return RequestContext{
		RequestID: fmt.Sprintf("request-%d", slot),
		TraceID:   fmt.Sprintf("%032x", slot),
		UserID:    fmt.Sprintf("user-%d", slot),
		TenantID:  fmt.Sprintf("tenant-%d", slot),
	}
}

func TestOperationBoundaryRecoversPanicAndKeepsListenerUsable(t *testing.T) {
	ctx := testOperationContext(1)
	reports := make([]OperationFailure, 0, 1)
	reporter := func(scoped context.Context, failure OperationFailure) {
		current, ok := CurrentContext(scoped)
		if !ok || current.RequestID != ctx.RequestID {
			t.Fatalf("reporter lost request context: %#v", current)
		}
		reports = append(reports, failure)
	}

	failed := RunOperationBoundary(
		context.Background(),
		ctx,
		OperationDescriptor{Transport: OperationTransportWebSocket, Scope: OperationScopeMessage, Name: "chat.message"},
		reporter,
		func(context.Context) (string, error) { panic("secret payload") },
	)
	if failed.OK() || failed.Failure.Kind != OperationFailurePanic {
		t.Fatalf("expected recovered panic, got %#v", failed)
	}
	if failed.Failure.RequestID != ctx.RequestID || failed.Failure.TraceID != ctx.TraceID {
		t.Fatalf("failure lost correlation: %#v", failed.Failure)
	}
	if len(reports) != 1 {
		t.Fatalf("expected one report, got %d", len(reports))
	}

	succeeded := RunOperationBoundary(
		context.Background(),
		ctx,
		OperationDescriptor{Transport: OperationTransportWebSocket, Scope: OperationScopeMessage, Name: "chat.message"},
		reporter,
		func(context.Context) (string, error) { return "ok", nil },
	)
	if !succeeded.OK() || succeeded.Value != "ok" {
		t.Fatalf("later message did not run: %#v", succeeded)
	}
}

func TestOperationBoundaryReporterPanicIsFailOpen(t *testing.T) {
	outcome := RunOperationBoundary(
		context.Background(),
		testOperationContext(2),
		OperationDescriptor{Transport: OperationTransportTCP, Scope: OperationScopeConnection, Name: "smtp.accept"},
		func(context.Context, OperationFailure) { panic("collector unavailable") },
		func(context.Context) (int, error) { return 0, errors.New("connection failed") },
	)
	if outcome.OK() || outcome.Failure.Kind != OperationFailureError {
		t.Fatalf("unexpected outcome: %#v", outcome)
	}
}

func TestOperationBoundaryClassifiesDeadlineAndCancellation(t *testing.T) {
	deadlineParent, cancelDeadline := context.WithTimeout(context.Background(), time.Nanosecond)
	defer cancelDeadline()
	time.Sleep(time.Millisecond)
	deadline := RunOperationBoundary(
		deadlineParent,
		testOperationContext(3),
		OperationDescriptor{Transport: OperationTransportHTTP, Scope: OperationScopeRequest, Name: "orders.read"},
		func(context.Context, OperationFailure) {},
		func(context.Context) (int, error) { return 1, nil },
	)
	if deadline.OK() || deadline.Failure.Kind != OperationFailureDeadlineExceeded {
		t.Fatalf("expected deadline failure: %#v", deadline)
	}

	cancelledParent, cancel := context.WithCancel(context.Background())
	cancel()
	cancelled := RunOperationBoundary(
		cancelledParent,
		testOperationContext(4),
		OperationDescriptor{Transport: OperationTransportTCP, Scope: OperationScopeCallback, Name: "tcp.read"},
		func(context.Context, OperationFailure) {},
		func(context.Context) (int, error) { return 1, nil },
	)
	if cancelled.OK() || cancelled.Failure.Kind != OperationFailureCancelled {
		t.Fatalf("expected cancellation failure: %#v", cancelled)
	}
}

func TestParallelOperationBoundariesDoNotBleed(t *testing.T) {
	const count = 64
	var wait sync.WaitGroup
	wait.Add(count)
	errors := make(chan error, count)

	for slot := 0; slot < count; slot++ {
		slot := slot
		go func() {
			defer wait.Done()
			ctx := testOperationContext(slot)
			outcome := RunOperationBoundary(
				context.Background(),
				ctx,
				OperationDescriptor{Transport: OperationTransportTCP, Scope: OperationScopeConnection, Name: "tcp.accept"},
				func(context.Context, OperationFailure) {},
				func(scoped context.Context) (int, error) {
					current, ok := CurrentContext(scoped)
					if !ok || current.RequestID != ctx.RequestID {
						return 0, fmt.Errorf("context bleed: want %s got %#v", ctx.RequestID, current)
					}
					return slot, nil
				},
			)
			if !outcome.OK() {
				errors <- fmt.Errorf("slot %d failed: %#v", slot, outcome.Failure)
			} else if outcome.Value != slot {
				errors <- fmt.Errorf("slot %d returned %d", slot, outcome.Value)
			}
		}()
	}
	wait.Wait()
	close(errors)
	for err := range errors {
		t.Error(err)
	}
}

func TestOperationBoundaryNormalizesUnboundedNames(t *testing.T) {
	outcome := RunOperationBoundary(
		context.Background(),
		testOperationContext(5),
		OperationDescriptor{Transport: OperationTransportTCP, Scope: OperationScopeCallback, Name: "customer/" + strings.Repeat("x", 200)},
		func(context.Context, OperationFailure) {},
		func(context.Context) (int, error) { return 0, errors.New("private payload") },
	)
	if outcome.OK() || outcome.Failure.Operation != "operation" {
		t.Fatalf("expected normalized name: %#v", outcome)
	}
}
