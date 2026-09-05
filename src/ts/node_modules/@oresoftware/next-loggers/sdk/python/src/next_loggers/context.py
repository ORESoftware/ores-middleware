"""Explicit contextvars and application-owned span adapters for next-loggers."""
from __future__ import annotations

import contextvars
import copy
import inspect
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, Iterator, Mapping, Optional, Protocol, Sequence, TypeVar

T = TypeVar("T")


def _copy_map(value: Optional[Mapping[str, Any]]) -> dict[str, Any]:
    return copy.deepcopy(dict(value or {}))


def _copy_users(value: Sequence[Mapping[str, Any]]) -> list[dict[str, Any]]:
    return [_copy_map(user) for user in value]


def _unique(values: Sequence[str]) -> tuple[str, ...]:
    result: list[str] = []
    for value in values:
        normalized = str(value or "").strip()
        if normalized and normalized not in result:
            result.append(normalized)
    return tuple(result)


@dataclass(frozen=True)
class LogContext:
    logged_in_user: Mapping[str, Any] = field(default_factory=dict)
    users: Sequence[Mapping[str, Any]] = field(default_factory=tuple)
    fields: Mapping[str, Any] = field(default_factory=dict)
    trace_id: str = ""
    trace_ids: Sequence[str] = field(default_factory=tuple)
    span_id: str = ""
    trace_flags: int = 0
    trace_state: str = ""
    remote: Optional[bool] = None
    baggage: Mapping[str, str] = field(default_factory=dict)
    routine_id: str = ""
    tags: Sequence[str] = field(default_factory=tuple)

    def snapshot(self) -> "LogContext":
        return LogContext(
            logged_in_user=_copy_map(self.logged_in_user),
            users=tuple(_copy_users(self.users)),
            fields=_copy_map(self.fields),
            trace_id=str(self.trace_id or ""),
            trace_ids=_unique(tuple(self.trace_ids)),
            span_id=str(self.span_id or ""),
            trace_flags=max(0, min(255, int(self.trace_flags))),
            trace_state=str(self.trace_state or "")[:512],
            remote=self.remote,
            baggage={str(key): str(value) for key, value in dict(self.baggage).items()},
            routine_id=str(self.routine_id or ""),
            tags=_unique(tuple(self.tags)),
        )


_EMPTY = LogContext()
_CURRENT: contextvars.ContextVar[LogContext] = contextvars.ContextVar(
    "next_loggers_context", default=_EMPTY
)


def merge_log_context(outer: LogContext, inner: LogContext) -> LogContext:
    outer = outer.snapshot()
    inner = inner.snapshot()
    logged_in_user = dict(outer.logged_in_user)
    logged_in_user.update(inner.logged_in_user)
    fields = dict(outer.fields)
    fields.update(inner.fields)
    baggage = dict(outer.baggage)
    baggage.update(inner.baggage)
    trace_id = inner.trace_id or outer.trace_id
    trace_ids = _unique((*outer.trace_ids, *inner.trace_ids, trace_id))
    return LogContext(
        logged_in_user=logged_in_user,
        users=(*outer.users, *inner.users),
        fields=fields,
        trace_id=trace_id,
        trace_ids=trace_ids,
        span_id=inner.span_id or outer.span_id,
        trace_flags=(
            inner.trace_flags
            if inner.trace_id or inner.span_id or inner.trace_flags
            else outer.trace_flags
        ),
        trace_state=inner.trace_state or outer.trace_state,
        remote=inner.remote if inner.remote is not None else outer.remote,
        baggage=baggage,
        routine_id=inner.routine_id or outer.routine_id,
        tags=_unique((*outer.tags, *inner.tags)),
    ).snapshot()


def get_log_context() -> LogContext:
    return _CURRENT.get().snapshot()


@contextmanager
def log_context(value: LogContext) -> Iterator[LogContext]:
    merged = merge_log_context(_CURRENT.get(), value)
    token = _CURRENT.set(merged)
    try:
        yield merged.snapshot()
    finally:
        _CURRENT.reset(token)


def run_with_log_context(value: LogContext, callback: Callable[..., T], *args: Any, **kwargs: Any) -> T:
    with log_context(value):
        result = callback(*args, **kwargs)
        if inspect.isawaitable(result):
            close = getattr(result, "close", None)
            if callable(close):
                close()
            raise TypeError("run_with_log_context received an awaitable; use run_with_log_context_async")
        return result


async def run_with_log_context_async(
    value: LogContext,
    callback: Callable[..., Awaitable[T]],
    *args: Any,
    **kwargs: Any,
) -> T:
    with log_context(value):
        return await callback(*args, **kwargs)


def update_log_context(update: Callable[[LogContext], LogContext]) -> LogContext:
    if not callable(update):
        raise TypeError("update_log_context requires a callable")
    updated = update(get_log_context())
    if not isinstance(updated, LogContext):
        raise TypeError("update_log_context callback must return LogContext")
    snapshot = updated.snapshot()
    _CURRENT.set(snapshot)
    return snapshot.snapshot()


def capture_log_context_callable(callback: Callable[..., T]) -> Callable[..., T]:
    """Capture context explicitly for an executor/thread boundary."""
    captured = contextvars.copy_context()

    def wrapped(*args: Any, **kwargs: Any) -> T:
        return captured.copy().run(callback, *args, **kwargs)

    return wrapped


def apply_log_context(event: Any, value: Optional[LogContext] = None) -> Any:
    context = (value or get_log_context()).snapshot()
    if context.trace_id:
        event.add_trace(context.trace_id, True)
    for trace_id in context.trace_ids:
        event.add_trace(trace_id)
    fields = dict(context.fields)
    if context.span_id:
        fields["otel.span_id"] = context.span_id
    fields["otel.trace_flags"] = context.trace_flags
    if context.trace_state:
        fields["otel.trace_state"] = context.trace_state
    if context.remote is not None:
        fields["otel.remote"] = context.remote
    if context.baggage:
        fields["otel.baggage"] = dict(context.baggage)
    event.add_fields(fields)
    if context.logged_in_user:
        event.add_logged_in_user_info(context.logged_in_user)
    for user in context.users:
        event.add_user_info(user)
    if context.routine_id:
        event.add_routine_id(context.routine_id)
    event.add_tags("otel", *context.tags)
    return event


class ContextLogger:
    """Small wrapper that applies the current context to every logger event."""

    def __init__(self, logger: Any) -> None:
        self.logger = logger

    def _event(self, method: str, values: Sequence[Any]) -> Any:
        return apply_log_context(getattr(self.logger, method)(*values))

    def trace(self, *values: Any) -> Any: return self._event("trace", values)
    def debug(self, *values: Any) -> Any: return self._event("debug", values)
    def info(self, *values: Any) -> Any: return self._event("info", values)
    def log(self, *values: Any) -> Any: return self.info(*values)
    def warn(self, *values: Any) -> Any: return self._event("warn", values)
    def error(self, *values: Any) -> Any: return self._event("error", values)
    def fatal(self, *values: Any) -> Any: return self._event("fatal", values)


class Span(Protocol):
    def log_context(self) -> LogContext: ...
    def is_recording(self) -> bool: ...
    def record_exception(self, error: BaseException) -> None: ...
    def set_status(self, code: int, description: str = "") -> None: ...
    def end(self) -> None: ...


class Tracer(Protocol):
    def start_span(self, name: str, attributes: Mapping[str, Any]) -> Span: ...


class _NoopSpan:
    def log_context(self) -> LogContext: return _EMPTY
    def is_recording(self) -> bool: return False
    def record_exception(self, error: BaseException) -> None: pass
    def set_status(self, code: int, description: str = "") -> None: pass
    def end(self) -> None: pass


def _bridge_log(event: Any) -> None:
    try:
        event.send()
    except BaseException:
        pass


def _recording(span: Span) -> bool:
    try:
        return bool(span.is_recording())
    except BaseException:
        return False


def _safe_span_call(logger: ContextLogger, operation: str, callback: Callable[[], None]) -> None:
    try:
        callback()
    except BaseException as error:
        _bridge_log(
            logger.warn("OpenTelemetry", operation, "failed", error)
            .add_fields({"otel.bridge_operation": operation})
            .add_tags("otel-bridge-error")
        )


def _start_span(logger: ContextLogger, tracer: Tracer, name: str, attributes: Mapping[str, Any]) -> Span:
    try:
        span = tracer.start_span(name, copy.deepcopy(dict(attributes)))
        return span if span is not None else _NoopSpan()
    except BaseException as error:
        _bridge_log(
            logger.error("OpenTelemetry start span failed", name, error)
            .add_fields({"otel.bridge_operation": "start span", "otel.span_name": name})
            .add_tags("otel-bridge-error")
        )
        return _NoopSpan()


def with_span(
    logger: Any,
    tracer: Tracer,
    name: str,
    callback: Callable[[Span], T],
    attributes: Optional[Mapping[str, Any]] = None,
) -> T:
    contextual = ContextLogger(logger)
    span = _start_span(contextual, tracer, name, attributes or {})
    try:
        span_context = span.log_context().snapshot()
    except BaseException:
        span_context = _EMPTY
    started = time.monotonic()
    with log_context(span_context):
        _bridge_log(contextual.debug("span started", name).add_fields({"otel.span_name": name, "otel.span_phase": "start"}))
        try:
            result = callback(span)
        except BaseException as error:
            if _recording(span):
                _safe_span_call(contextual, "record exception", lambda: span.record_exception(error))
                _safe_span_call(contextual, "set error status", lambda: span.set_status(2, str(error)))
            _bridge_log(contextual.error("span failed", name, error).add_fields({"otel.span_name": name, "otel.span_phase": "error", "otel.duration_ms": (time.monotonic() - started) * 1000}))
            raise
        else:
            if _recording(span):
                _safe_span_call(contextual, "set success status", lambda: span.set_status(1, ""))
            _bridge_log(contextual.debug("span completed", name).add_fields({"otel.span_name": name, "otel.span_phase": "end", "otel.duration_ms": (time.monotonic() - started) * 1000}))
            return result
        finally:
            _safe_span_call(contextual, "end span", span.end)


async def with_span_async(
    logger: Any,
    tracer: Tracer,
    name: str,
    callback: Callable[[Span], Awaitable[T]],
    attributes: Optional[Mapping[str, Any]] = None,
) -> T:
    contextual = ContextLogger(logger)
    span = _start_span(contextual, tracer, name, attributes or {})
    try:
        span_context = span.log_context().snapshot()
    except BaseException:
        span_context = _EMPTY
    started = time.monotonic()
    with log_context(span_context):
        _bridge_log(contextual.debug("span started", name).add_fields({"otel.span_name": name, "otel.span_phase": "start"}))
        try:
            result = await callback(span)
        except BaseException as error:
            if _recording(span):
                _safe_span_call(contextual, "record exception", lambda: span.record_exception(error))
                _safe_span_call(contextual, "set error status", lambda: span.set_status(2, str(error)))
            _bridge_log(contextual.error("span failed", name, error).add_fields({"otel.span_name": name, "otel.span_phase": "error", "otel.duration_ms": (time.monotonic() - started) * 1000}))
            raise
        else:
            if _recording(span):
                _safe_span_call(contextual, "set success status", lambda: span.set_status(1, ""))
            _bridge_log(contextual.debug("span completed", name).add_fields({"otel.span_name": name, "otel.span_phase": "end", "otel.duration_ms": (time.monotonic() - started) * 1000}))
            return result
        finally:
            _safe_span_call(contextual, "end span", span.end)


__all__ = [
    "ContextLogger", "LogContext", "Span", "Tracer", "apply_log_context",
    "capture_log_context_callable", "get_log_context", "log_context",
    "merge_log_context", "run_with_log_context", "run_with_log_context_async",
    "update_log_context", "with_span", "with_span_async",
]
