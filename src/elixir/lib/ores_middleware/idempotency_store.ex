defmodule OresMiddleware.IdempotencyStore do
  @moduledoc false
  use GenServer

  def start_link(opts), do: GenServer.start_link(__MODULE__, %{}, opts)
  def get(server \\ __MODULE__, key), do: GenServer.call(server, {:get, key})
  def put(server \\ __MODULE__, key, response, ttl_seconds), do: GenServer.call(server, {:put, key, response, ttl_seconds})

  @impl true
  def init(state), do: {:ok, state}

  @impl true
  def handle_call({:get, key}, _from, state) do
    now = System.monotonic_time(:millisecond)
    case Map.get(state, key) do
      %{expires_at: expires_at, response: response} when expires_at > now -> {:reply, {:ok, response}, state}
      nil -> {:reply, :miss, state}
      _expired -> {:reply, :miss, Map.delete(state, key)}
    end
  end

  @impl true
  def handle_call({:put, key, response, ttl_seconds}, _from, state) do
    expires_at = System.monotonic_time(:millisecond) + ttl_seconds * 1_000
    {:reply, :ok, Map.put(state, key, %{expires_at: expires_at, response: response})}
  end
end
