defmodule OresMiddleware.Operation do
  @moduledoc """
  Isolated protocol-operation boundary with bounded public failures.

  Raw exception messages, stack traces, request bodies, credentials, and
  identities are never copied into the returned failure. Reporter failure is
  explicitly fail-open, while request/log context restoration is delegated to
  `OresMiddleware.Context.run/2` and therefore always runs.
  """

  require Logger
  alias OresMiddleware.Context

  @safe_token ~r/^[A-Za-z0-9_.:-]+$/

  @type failure_kind :: :error | :panic | :cancelled | :deadline_exceeded
  @type descriptor :: %{
          required(:transport) => atom() | String.t(),
          required(:scope) => atom() | String.t(),
          required(:name) => String.t(),
          optional(:cancelled) => boolean(),
          optional(:deadline_unix_ms) => integer()
        }
  @type failure :: %{
          required(:kind) => failure_kind(),
          required(:code) => String.t(),
          required(:transport) => atom() | String.t(),
          required(:scope) => atom() | String.t(),
          required(:operation) => String.t(),
          required(:request_id) => String.t() | nil,
          required(:trace_id) => String.t() | nil,
          required(:error_type) => String.t()
        }

  @spec run(map(), descriptor(), (-> result), (failure() -> any())) ::
          {:ok, result} | {:error, failure()}
        when result: var
  def run(context, descriptor, operation, reporter \\ &default_reporter/1)
      when is_map(context) and is_map(descriptor) and is_function(operation, 0) and
             is_function(reporter, 1) do
    descriptor = normalize_descriptor(descriptor)

    Context.run(context, fn ->
      case preflight_failure(descriptor) do
        nil -> invoke(context, descriptor, operation, reporter)
        kind -> fail(context, descriptor, kind, Atom.to_string(kind), reporter)
      end
    end)
  end

  defp invoke(context, descriptor, operation, reporter) do
    try do
      {:ok, operation.()}
    rescue
      error -> fail(context, descriptor, :error, safe_error_type(error), reporter)
    catch
      class, _reason -> fail(context, descriptor, :panic, safe_token(class, "panic", 64), reporter)
    end
  end

  defp preflight_failure(%{cancelled: true}), do: :cancelled

  defp preflight_failure(%{deadline_unix_ms: deadline}) when is_integer(deadline) do
    if deadline <= System.system_time(:millisecond), do: :deadline_exceeded
  end

  defp preflight_failure(_descriptor), do: nil

  defp fail(context, descriptor, kind, error_type, reporter) do
    failure = %{
      kind: kind,
      code: failure_code(kind),
      transport: descriptor.transport,
      scope: descriptor.scope,
      operation: descriptor.name,
      request_id: Map.get(context, :request_id),
      trace_id: Map.get(context, :trace_id),
      error_type: safe_token(error_type, "error", 64)
    }

    report_safely(reporter, failure)
    {:error, failure}
  end

  defp report_safely(reporter, failure) do
    try do
      reporter.(failure)
    rescue
      _error -> :ok
    catch
      _class, _reason -> :ok
    end
  end

  defp default_reporter(failure) do
    Logger.error("operation failed",
      operation_name: failure.operation,
      operation_transport: failure.transport,
      operation_scope: failure.scope,
      operation_outcome: failure.kind,
      error_type: failure.error_type,
      request_id: failure.request_id,
      trace_id: failure.trace_id
    )
  end

  defp normalize_descriptor(descriptor) do
    %{
      transport: Map.get(descriptor, :transport, :http),
      scope: Map.get(descriptor, :scope, :request),
      name: safe_token(Map.get(descriptor, :name), "operation", 128),
      cancelled: Map.get(descriptor, :cancelled, false),
      deadline_unix_ms: Map.get(descriptor, :deadline_unix_ms)
    }
  end

  defp safe_error_type(%{__struct__: module}) when is_atom(module) do
    module
    |> Module.split()
    |> List.last()
    |> safe_token("error", 64)
  end

  defp safe_error_type(_error), do: "error"

  defp safe_token(value, fallback, maximum) when is_atom(value) do
    value |> Atom.to_string() |> safe_token(fallback, maximum)
  end

  defp safe_token(value, fallback, maximum) when is_binary(value) do
    if byte_size(value) in 1..maximum and Regex.match?(@safe_token, value), do: value, else: fallback
  end

  defp safe_token(_value, fallback, _maximum), do: fallback

  defp failure_code(:error), do: "operation_failed"
  defp failure_code(:panic), do: "operation_panicked"
  defp failure_code(:cancelled), do: "operation_cancelled"
  defp failure_code(:deadline_exceeded), do: "operation_deadline_exceeded"
end
