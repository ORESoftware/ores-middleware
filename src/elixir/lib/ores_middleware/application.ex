defmodule OresMiddleware.Application do
  @moduledoc false
  use Application

  @impl true
  def start(_type, _args) do
    children = [
      {OresMiddleware.TokenBucket, name: OresMiddleware.TokenBucket},
      {OresMiddleware.IdempotencyStore, name: OresMiddleware.IdempotencyStore}
    ]

    Supervisor.start_link(children, strategy: :one_for_one, name: OresMiddleware.Supervisor)
  end
end
