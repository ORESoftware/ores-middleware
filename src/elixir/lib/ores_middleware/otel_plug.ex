defmodule OresMiddleware.OTelPlug do
  @moduledoc """
  Plug/Phoenix adapter that composes after `OresMiddleware.Plug`.

  It creates a request-specific logger at `conn.assigns.log` and installs the
  canonical ores-otel process context so separately imported file loggers see
  the same request, trace, user, and tenant identifiers.
  """

  @behaviour Plug
  import Plug.Conn

  alias OresMiddleware.OTel

  @impl true
  def init(opts) do
    %{
      logger: Keyword.fetch!(opts, :logger),
      strict: Keyword.get(opts, :strict, true),
      lifecycle: Keyword.get(opts, :lifecycle, true)
    }
  end

  @impl true
  def call(conn, options) do
    case conn.assigns[:ores_middleware_context] do
      nil when options.strict ->
        raise ArgumentError,
              "OresMiddleware.OTelPlug must run after OresMiddleware.Plug"

      nil ->
        conn

      context ->
        root = resolve_logger(options.logger)
        request_logger = OTel.request_logger(root, context)
        restore_token = OTel.put_context(context)

        if options.lifecycle do
          OTel.info(request_logger, "request handler started", %{
            "http.request.method" => conn.method,
            "url.path" => conn.request_path
          })
        end

        conn
        |> assign(:log, request_logger)
        |> assign(:ores_log, request_logger)
        |> register_before_send(fn conn ->
          try do
            if options.lifecycle do
              OTel.info(request_logger, "request handler completed", %{
                "http.request.method" => conn.method,
                "url.path" => conn.request_path,
                "http.response.status_code" => conn.status
              })
            end

            conn
          after
            OTel.restore_context(restore_token)
          end
        end)
    end
  end

  defp resolve_logger(provider) when is_function(provider, 0), do: provider.()
  defp resolve_logger(logger) when is_map(logger), do: logger

  defp resolve_logger(other) do
    raise ArgumentError,
          "expected :logger to be an ores-otel logger map or zero-arity provider, got: #{inspect(other)}"
  end
end
