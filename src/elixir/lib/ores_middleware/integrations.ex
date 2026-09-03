defmodule OresMiddleware.Integrations do
  @moduledoc "HTTP hooks for shared-auth and opto-sync. Endpoints come from validated configuration; no credentials are embedded."

  def shared_auth_http(config) do
    fn conn, _context ->
      authorization = conn |> Plug.Conn.get_req_header("authorization") |> List.first()
      if is_nil(authorization), do: {:ok, %{user_id: nil, tenant_id: nil, baggage: %{}}}, else: introspect(config, authorization)
    end
  end

  def opto_sync_http(config) do
    fn context, conn, duration_ms ->
      body = Jason.encode!(%{topic: config.outboxTopic, requestId: context.request_id, traceId: context.trace_id, method: conn.method, path: conn.request_path, status: conn.status, durationMs: duration_ms})
      case post_json(config.endpoint, body, []) do
        {:ok, status, _} when status in 200..299 -> :ok
        other -> {:error, other}
      end
    end
  end

  defp introspect(config, authorization) do
    body = Jason.encode!(%{authorization: authorization, audience: config.audience})
    case post_json(config.introspectionUrl, body, []) do
      {:ok, status, payload} when status in 200..299 ->
        case Jason.decode(payload) do
          {:ok, %{"active" => true, "sub" => subject} = value} -> {:ok, %{user_id: subject, tenant_id: value["tenantId"], baggage: value["claims"] || %{}}}
          _ -> {:ok, %{user_id: nil, tenant_id: nil, baggage: %{}}}
        end
      other -> {:error, other}
    end
  end

  defp post_json(nil, _body, _headers), do: {:error, :missing_endpoint}
  defp post_json(endpoint, body, headers) do
    request = {String.to_charlist(endpoint), [{'content-type', 'application/json'} | headers], 'application/json', body}
    case :httpc.request(:post, request, [timeout: 5_000, connect_timeout: 2_000], body_format: :binary) do
      {:ok, {{_, status, _}, _headers, response_body}} -> {:ok, status, response_body}
      error -> error
    end
  end
end
