defmodule OresMiddleware.OTel do
  @moduledoc """
  Bridges the portable ORES request context to `ORESoftware.NextLoggers`.

  The portable context remains data-only. A request-specific logger map is
  attached to `conn.assigns.log`, while ordinary file/module loggers inherit
  correlation from process-local context for the lifetime of the request.
  """

  require Logger
  alias ORESoftware.NextLoggers

  def new_logger(app_name, opts \\ []), do: NextLoggers.new(app_name, opts)
  defdelegate otel_transport(sink), to: NextLoggers
  defdelegate supabase_transport(sender), to: NextLoggers
  defdelegate use_otel(logger), to: NextLoggers
  defdelegate not_otel(logger), to: NextLoggers

  def to_log_context(context) when is_map(context) do
    fields =
      %{}
      |> put_optional("request.id", Map.get(context, :request_id))
      |> put_optional("trace.id", Map.get(context, :trace_id))
      |> put_optional(
        "request.started_at_unix_ms",
        Map.get(context, :started_at_unix_ms)
      )
      |> put_optional(
        "request.deadline_unix_ms",
        Map.get(context, :deadline_unix_ms)
      )
      |> put_optional("user.id", Map.get(context, :user_id))
      |> put_optional("tenant.id", Map.get(context, :tenant_id))
      |> put_optional("request.locale", Map.get(context, :locale))
      |> put_otel_baggage(Map.get(context, :baggage, %{}))

    %{
      trace_id: Map.get(context, :trace_id),
      span_id: Map.get(context, :span_id),
      trace_flags: Map.get(context, :trace_flags, 0),
      trace_state: Map.get(context, :trace_state),
      fields: fields,
      tags: ["ores-middleware", "request"]
    }
  end

  def put_context(context) when is_map(context) do
    context
    |> to_log_context()
    |> NextLoggers.put_context()
  end

  def restore_context(token), do: NextLoggers.restore_context(token)

  def with_context(context, operation) when is_map(context) and is_function(operation, 0) do
    NextLoggers.with_context(to_log_context(context), operation)
  end

  def request_logger(nil, _context), do: nil

  def request_logger(logger, context) when is_map(logger) and is_map(context) do
    log_context = to_log_context(context)
    logger_name = Map.get(logger, :name)
    name = if logger_name, do: "#{logger_name}:request", else: "request"

    pinned =
      logger
      |> Map.put(:name, name)
      |> Map.put(
        :fields,
        Map.merge(Map.get(logger, :fields, %{}), log_context.fields)
      )
      |> Map.put(:ores_request_context, log_context)

    Map.merge(pinned, %{
      info: fn message -> info(pinned, message) end,
      warn: fn message -> warn(pinned, message) end,
      error: fn message -> error(pinned, message) end,
      info_fields: fn message, fields -> info(pinned, message, fields) end,
      warn_fields: fn message, fields -> warn(pinned, message, fields) end,
      error_fields: fn message, fields -> error(pinned, message, fields) end
    })
  end

  def assign_request_logger(conn, nil, _context), do: conn

  def assign_request_logger(conn, logger, context) do
    request_logger = request_logger(logger, context)

    conn
    |> Plug.Conn.assign(:log, request_logger)
    |> Plug.Conn.assign(:ores_log, request_logger)
  end

  def info(logger, message, fields \\ %{}), do: log(logger, "INFO", message, fields)
  def warn(logger, message, fields \\ %{}), do: log(logger, "WARN", message, fields)
  def error(logger, message, fields \\ %{}), do: log(logger, "ERROR", message, fields)

  def log(nil, _level, _message, _fields), do: :disabled

  def log(logger, level, message, fields)
      when is_map(logger) and is_binary(level) and is_binary(message) and is_map(fields) do
    context = Map.get(logger, :ores_request_context, %{})

    try do
      record =
        NextLoggers.with_context(context, fn ->
          NextLoggers.log(logger, level, message, fields)
        end)

      {:ok, record}
    rescue
      exception ->
        Logger.warning("ores request log delivery failed",
          level: level,
          error: Exception.message(exception)
        )

        {:error, exception}
    end
  end

  defp put_optional(fields, _key, value) when value in [nil, ""], do: fields
  defp put_optional(fields, key, value), do: Map.put(fields, key, value)

  defp put_otel_baggage(fields, baggage) do
    baggage =
      baggage
      |> Enum.filter(fn {key, _value} -> String.starts_with?(to_string(key), "otel.") end)
      |> Map.new(fn {key, value} -> {to_string(key), to_string(value)} end)

    if map_size(baggage) == 0,
      do: fields,
      else: Map.put(fields, "otel.baggage", baggage)
  end
end
