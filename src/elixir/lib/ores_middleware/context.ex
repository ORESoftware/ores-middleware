defmodule OresMiddleware.Context do
  @moduledoc false
  require Logger

  @key :ores_middleware_request_context

  def current, do: Process.get(@key)

  def put(context) when is_map(context) do
    Process.put(@key, context)

    Logger.metadata(
      request_id: context.request_id,
      trace_id: context.trace_id,
      tenant_id: context.tenant_id,
      user_id: context.user_id
    )

    :ok
  end

  def clear do
    Process.delete(@key)
    :ok
  end

  def run(context, operation) when is_function(operation, 0) do
    previous_context = current()
    previous_metadata = Logger.metadata()
    put(context)

    try do
      OresMiddleware.OTel.with_context(context, operation)
    after
      case previous_context do
        nil -> clear()
        value -> Process.put(@key, value)
      end

      Logger.reset_metadata(previous_metadata)
    end
  end
end
