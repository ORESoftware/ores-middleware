defmodule OresMiddleware.TokenBucket do
  @moduledoc false
  use GenServer

  def start_link(opts), do: GenServer.start_link(__MODULE__, %{}, opts)
  def allow(server \\ __MODULE__, key, capacity, refill_per_second), do: GenServer.call(server, {:allow, key, capacity, refill_per_second})

  @impl true
  def init(state), do: {:ok, state}

  @impl true
  def handle_call({:allow, key, capacity, refill}, _from, state) do
    now = System.monotonic_time(:microsecond)
    %{tokens: tokens, updated: updated} = Map.get(state, key, %{tokens: capacity * 1.0, updated: now})
    tokens = min(capacity * 1.0, tokens + (now - updated) / 1_000_000 * refill)
    allowed = tokens >= 1.0
    tokens = if allowed, do: tokens - 1.0, else: tokens
    {:reply, allowed, Map.put(state, key, %{tokens: tokens, updated: now})}
  end
end
