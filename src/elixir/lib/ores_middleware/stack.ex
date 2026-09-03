defmodule OresMiddleware.Stack do
  @moduledoc false
  require Logger

  defstruct [:config, :hooks]

  def new(config, hooks \\ %{}) do
    case OresMiddleware.Config.validate(config) do
      [] -> {:ok, %__MODULE__{config: config, hooks: Map.merge(default_hooks(), Map.new(hooks))}}
      issues -> {:error, issues}
    end
  end

  def new!(config, hooks \\ %{}) do
    case new(config, hooks) do
      {:ok, stack} -> stack
      {:error, issues} -> raise ArgumentError, "invalid middleware configuration: #{inspect(issues)}"
    end
  end

  def default_hooks do
    %{
      authenticate: fn _conn, _context -> {:ok, %{user_id: nil, tenant_id: nil, baggage: %{}}} end,
      resolve_test_identity: fn _conn, _context -> {:error, :not_configured} end,
      authorize_ip: fn _conn, _context -> true end,
      rate_limit: fn key, capacity, refill -> OresMiddleware.TokenBucket.allow(key, capacity, refill) end,
      idempotency_get: fn key -> OresMiddleware.IdempotencyStore.get(key) end,
      idempotency_put: fn key, response, ttl -> OresMiddleware.IdempotencyStore.put(key, response, ttl) end,
      trusted_proxy?: fn conn, cidrs -> OresMiddleware.IP.in_cidrs?(conn.remote_ip, cidrs) end,
      telemetry_started: fn context, conn -> Logger.info("request started", request_id: context.request_id, trace_id: context.trace_id, method: conn.method, path: conn.request_path) end,
      telemetry_finished: fn context, conn, duration_ms -> Logger.info("request finished", request_id: context.request_id, trace_id: context.trace_id, status: conn.status, duration_ms: duration_ms) end,
      sync_observe: fn _context, _conn, _duration_ms -> :ok end,
      schema_capture: fn _conn -> :ok end
    }
  end
end
