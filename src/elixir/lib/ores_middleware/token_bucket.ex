defmodule OresMiddleware.TokenBucket do
  @moduledoc false
  use GenServer

  @max_entries 10_000

  def start_link(opts), do: GenServer.start_link(__MODULE__, initial_state(), opts)

  def allow(server \\ __MODULE__, key, capacity, refill_per_second),
    do: GenServer.call(server, {:allow, key, capacity, refill_per_second})

  @impl true
  def init(_state), do: {:ok, initial_state()}

  @impl true
  def handle_call({:allow, key, capacity, refill}, _from, state) do
    now = System.monotonic_time(:microsecond)
    state = normalize_state(state)
    {state, bucket} = ensure_bucket(state, key, capacity, now)
    %{tokens: tokens, updated: updated} = bucket

    tokens = min(capacity * 1.0, tokens + (now - updated) / 1_000_000 * refill)
    allowed = tokens >= 1.0
    tokens = if allowed, do: tokens - 1.0, else: tokens
    buckets = Map.put(state.buckets, key, %{tokens: tokens, updated: now})
    {:reply, allowed, %{state | buckets: buckets}}
  end

  defp initial_state, do: %{buckets: %{}, order: :queue.new()}

  defp normalize_state(%{buckets: buckets, order: order} = state)
       when is_map(buckets) and is_tuple(order),
       do: state

  defp normalize_state(legacy) when is_map(legacy) do
    # One-time hot-upgrade path from the previous unbounded bucket map.
    keys = legacy |> Map.keys() |> Enum.take(@max_entries)
    %{buckets: Map.take(legacy, keys), order: :queue.from_list(keys)}
  end

  defp ensure_bucket(state, key, capacity, now) do
    case Map.fetch(state.buckets, key) do
      {:ok, bucket} ->
        {state, bucket}

      :error ->
        # HOT-PATH: FIFO eviction is O(1) and bounds attacker-controlled IP
        # cardinality without a full-map scan on every new key.
        state = evict_if_full(state)
        bucket = %{tokens: capacity * 1.0, updated: now}

        {
          %{
            state
            | buckets: Map.put(state.buckets, key, bucket),
              order: :queue.in(key, state.order)
          },
          bucket
        }
    end
  end

  defp evict_if_full(%{buckets: buckets} = state) when map_size(buckets) < @max_entries,
    do: state

  defp evict_if_full(state) do
    case :queue.out(state.order) do
      {{:value, oldest}, order} ->
        %{state | buckets: Map.delete(state.buckets, oldest), order: order}

      {:empty, _} ->
        state
    end
  end
end
