defmodule OresMiddleware.Plug do
  @moduledoc "Plug/Phoenix adapter for the ORES middleware contract."
  @behaviour Plug

  import Plug.Conn
  require Logger

  alias OresMiddleware.{Context, Stack}

  @zero_trace_id String.duplicate("0", 32)
  @zero_span_id String.duplicate("0", 16)

  @impl true
  def init(opts) do
    config = Keyword.get_lazy(opts, :config, fn -> OresMiddleware.default_config(Keyword.get(opts, :service_name, "ores-service")) end)
    Stack.new!(config, Keyword.get(opts, :hooks, %{}))
  end

  @impl true
  def call(conn, %Stack{} = stack) do
    case begin_request(conn, stack) do
      {:ok, conn, context, started_ms, idempotency_key, previous_context, previous_metadata} ->
        Context.put(context)
        stack.hooks.telemetry_started.(context, conn)

        register_before_send(conn, fn conn ->
          finish_request(conn, stack, context, started_ms, idempotency_key)
          |> restore_process(previous_context, previous_metadata)
        end)

      {:halt, conn} -> conn
    end
  end

  def wrap(%Stack{} = stack, conn, next) when is_function(next, 1) do
    case begin_request(conn, stack) do
      {:halt, conn} -> conn
      {:ok, conn, context, started_ms, idempotency_key, previous_context, previous_metadata} ->
        parent = self()
        tag = make_ref()

        {pid, monitor} = Process.spawn(fn ->
          result =
            try do
              {:ok, Context.run(context, fn -> next.(conn) end)}
            rescue
              error ->
                Logger.error("request handler failed", request_id: context.request_id, trace_id: context.trace_id, error: Exception.message(error))
                {:error, :handler_failed}
            catch
              _kind, _reason -> {:error, :handler_failed}
            end

          send(parent, {tag, result})
        end, [:monitor])

        result = receive do
          {^tag, result} ->
            Process.demonitor(monitor, [:flush])
            result
          {:DOWN, ^monitor, :process, ^pid, _reason} -> {:error, :handler_failed}
        after
          stack.config.settings.timeoutMs ->
            Process.exit(pid, :kill)
            {:error, :deadline_exceeded}
        end

        response = case result do
          {:ok, conn} -> conn
          {:error, :deadline_exceeded} -> problem(conn, 504, "deadline_exceeded", "request deadline exceeded")
          {:error, _} -> problem(conn, 500, "internal_error", "request handler failed")
        end

        finish_request(response, stack, context, started_ms, idempotency_key)
        |> restore_process(previous_context, previous_metadata)
    end
  end

  defp begin_request(conn, stack) do
    config = stack.config
    previous_context = Context.current()
    previous_metadata = Logger.metadata()

    with :ok <- check_payload(conn, config),
         :ok <- check_accept(conn, config),
         {:ok, trusted_proxy} <- check_transport(conn, stack),
         context <- build_context(conn, config),
         :ok <- authorize_ip(conn, context, stack),
         {:ok, context} <- authenticate(conn, context, stack),
         :ok <- rate_limit(conn, context, stack, trusted_proxy),
         :ok <- inject_fault(config),
         {:continue, conn, idempotency_key} <- idempotency_lookup(conn, stack) do
      {:ok, assign(conn, :ores_middleware_context, context), context, System.monotonic_time(:millisecond), idempotency_key, previous_context, previous_metadata}
    else
      {:cached, conn} -> {:halt, conn}
      {:error, status, code, detail} -> {:halt, problem(conn, status, code, detail)}
    end
  end

  defp check_payload(conn, config) do
    case first_req_header(conn, "content-length") do
      nil -> :ok
      value -> case Integer.parse(value) do
        {length, ""} when length <= config.settings.maxBodyBytes -> :ok
        {_, ""} -> {:error, 413, "payload_too_large", "request body exceeds configured limit"}
        _ -> {:error, 400, "invalid_content_length", "content-length is invalid"}
      end
    end
  end

  defp check_accept(conn, config) do
    accept = first_req_header(conn, "accept")
    if is_nil(accept) or accept == "*/*" or Enum.any?(config.settings.contentRepresentations, &String.contains?(accept, &1)), do: :ok, else: {:error, 406, "not_acceptable", "no supported representation was requested"}
  end

  defp check_transport(conn, stack) do
    tls = stack.config.settings.tls
    trusted = stack.hooks.trusted_proxy?.(conn, tls.trustedProxyCidrs)
    forwarded = first_req_header(conn, "x-forwarded-proto")
    cond do
      tls.strictForwardedHeaders and not is_nil(forwarded) and not trusted -> {:error, 400, "untrusted_forwarded_header", "forwarded transport headers came from an untrusted peer"}
      tls.requireHttps and conn.scheme != :https and not (trusted and forwarded == "https") -> {:error, 426, "https_required", "HTTPS is required"}
      true -> {:ok, trusted}
    end
  end

  defp build_context(conn, config) do
    started = System.system_time(:millisecond)
    request_id = valid_token(first_req_header(conn, config.settings.requestIdHeader)) || random_id()
    trace_id = parse_trace_id(first_req_header(conn, config.settings.traceHeader)) || random_id()
    %{request_id: request_id, trace_id: trace_id, tenant_id: nil, user_id: nil, locale: first_req_header(conn, "accept-language"), started_at_unix_ms: started, deadline_unix_ms: started + config.settings.timeoutMs, baggage: %{}}
  end

  defp authorize_ip(conn, context, stack), do: if(stack.hooks.authorize_ip.(conn, context), do: :ok, else: {:error, 403, "ip_policy_denied", "request source is not permitted"})

  defp authenticate(conn, context, stack) do
    bypass = stack.config.settings.testAuthBypass.enabled and first_req_header(conn, stack.config.settings.testAuthBypass.headerName) == "true"
    result = cond do
      bypass and stack.config.environment in [:test, :staging] -> stack.hooks.resolve_test_identity.(conn, context)
      bypass -> {:error, :production_forbidden}
      true -> stack.hooks.authenticate.(conn, context)
    end

    case result do
      {:ok, decision} ->
        context = %{context | user_id: decision[:user_id], tenant_id: decision[:tenant_id], baggage: Map.new(decision[:baggage] || %{})}
        if stack.config.integrations.sharedAuth.mode != :disabled and is_nil(context.user_id), do: {:error, 401, "authentication_required", "shared-auth did not establish a user"}, else: {:ok, context}
      _ -> {:error, 401, "authentication_failed", "authentication failed"}
    end
  end

  defp rate_limit(conn, context, stack, trusted_proxy) do
    policy = stack.config.settings.rateLimit
    key = Enum.join([context.tenant_id || "_", context.user_id || "_", client_ip(conn, trusted_proxy), conn.request_path], ":")
    if not policy.enabled or stack.hooks.rate_limit.(key, policy.capacity, policy.refillPerSecond), do: :ok, else: {:error, 429, "rate_limited", "rate limit exceeded"}
  end

  defp inject_fault(config) do
    policy = config.settings.faultInjection
    if policy.enabled and policy.latencyMs > 0, do: Process.sleep(policy.latencyMs)
    cond do
      policy.enabled and :rand.uniform() < policy.dropRate -> {:error, 503, "fault_drop", "injected transport drop"}
      policy.enabled and :rand.uniform() < policy.errorRate -> {:error, 500, "fault_error", "injected middleware error"}
      true -> :ok
    end
  end

  defp idempotency_lookup(conn, stack) do
    policy = stack.config.settings.idempotency
    key = first_req_header(conn, policy.headerName)
    if policy.enabled and conn.method in policy.requiredMethods and is_binary(key) and key != "" do
      cache_key = Enum.join([conn.method, conn.request_path, key], ":")
      case stack.hooks.idempotency_get.(cache_key) do
        {:ok, %{status: status, headers: headers, body: body}} -> {:cached, conn |> merge_resp_headers(headers) |> send_resp(status, body) |> halt()}
        _ -> {:continue, conn, cache_key}
      end
    else
      {:continue, conn, nil}
    end
  end

  defp finish_request(conn, stack, context, started_ms, idempotency_key) do
    duration = max(0, System.monotonic_time(:millisecond) - started_ms)
    conn = conn |> attach_correlation(stack.config, context) |> attach_security(stack.config) |> attach_etag() |> maybe_compress(stack.config)
    stack.hooks.schema_capture.(conn)

    conn = case stack.hooks.sync_observe.(context, conn, duration) do
      :ok -> conn
      {:error, _} when stack.config.integrations.optoSync.failOpen -> conn
      {:error, _} -> problem(conn, 503, "sync_observer_failed", "opto-sync observation failed")
    end

    if idempotency_key && conn.status in 200..299 do
      stack.hooks.idempotency_put.(idempotency_key, %{status: conn.status, headers: conn.resp_headers, body: conn.resp_body || ""}, stack.config.settings.idempotency.ttlSeconds)
    end

    stack.hooks.telemetry_finished.(context, conn, duration)
    conn
  end

  defp attach_correlation(conn, config, context) do
    conn
    |> put_resp_header(config.settings.requestIdHeader, context.request_id)
    |> sanitize_traceparent()
    |> update_resp_header("vary", "accept", &(&1 <> ", accept"))
  end

  defp attach_security(conn, %{settings: %{securityHeaders: %{enabled: false}}}), do: conn
  defp attach_security(conn, config) do
    policy = config.settings.securityHeaders
    conn
    |> put_resp_header("x-content-type-options", "nosniff")
    |> put_resp_header("x-frame-options", policy.frameOptions)
    |> put_resp_header("referrer-policy", "strict-origin-when-cross-origin")
    |> put_resp_header("strict-transport-security", "max-age=#{policy.hstsMaxAgeSeconds}; includeSubDomains")
    |> maybe_put_header("content-security-policy", policy.contentSecurityPolicy)
  end

  defp attach_etag(%{method: "GET", status: 200, resp_body: body} = conn) when is_binary(body) do
    etag = "\"" <> Base.encode16(:crypto.hash(:sha256, body), case: :lower) <> "\""
    if first_req_header(conn, "if-none-match") == etag, do: %{put_resp_header(conn, "etag", etag) | status: 304, resp_body: ""}, else: put_resp_header(conn, "etag", etag)
  end
  defp attach_etag(conn), do: conn

  defp maybe_compress(%{resp_body: body} = conn, config) when is_binary(body) do
    policy = config.settings.compression
    if policy.enabled and byte_size(body) >= policy.minimumBytes and String.contains?(first_req_header(conn, "accept-encoding") || "", "gzip") and is_nil(get_resp_header(conn, "content-encoding") |> List.first()) do
      conn |> put_resp_header("content-encoding", "gzip") |> delete_resp_header("content-length") |> update_resp_header("vary", "accept-encoding", &(&1 <> ", accept-encoding")) |> Map.put(:resp_body, :zlib.gzip(body))
    else
      conn
    end
  end
  defp maybe_compress(conn, _config), do: conn

  defp restore_process(conn, previous_context, previous_metadata) do
    case previous_context do nil -> Context.clear(); value -> Context.put(value) end
    Logger.reset_metadata(previous_metadata)
    conn
  end

  defp problem(conn, status, code, detail) do
    body = Jason.encode!(%{type: "urn:ores:middleware:#{code}", title: code, status: status, detail: detail})
    conn |> put_resp_content_type("application/problem+json") |> resp(status, body) |> halt()
  end

  defp first_req_header(conn, name), do: conn |> get_req_header(String.downcase(name)) |> List.first()
  defp valid_token(value) when is_binary(value), do: if(byte_size(value) <= 128 and Regex.match?(~r/^[A-Za-z0-9._-]+$/, value), do: value)
  defp valid_token(_), do: nil
  defp parse_trace_id(value) when is_binary(value) do
    case String.split(value, "-") do
      [_, trace | _] ->
        trace = String.downcase(trace)
        if valid_hex_id?(trace, 32, @zero_trace_id), do: trace
      _ -> nil
    end
  end
  defp parse_trace_id(_), do: nil

  defp sanitize_traceparent(conn) do
    case get_resp_header(conn, "traceparent") do
      [value | _] ->
        case normalize_traceparent(value) do
          nil -> delete_resp_header(conn, "traceparent")
          normalized -> put_resp_header(conn, "traceparent", normalized)
        end
      [] -> conn
    end
  end

  defp normalize_traceparent(value) when is_binary(value) do
    case String.split(value, "-") do
      [version, trace, span, flags] ->
        version = String.downcase(version)
        trace = String.downcase(trace)
        span = String.downcase(span)
        flags = String.downcase(flags)

        if version == "00" and
             valid_hex_id?(trace, 32, @zero_trace_id) and
             valid_hex_id?(span, 16, @zero_span_id) and
             Regex.match?(~r/^[0-9a-f]{2}$/, flags) do
          Enum.join([version, trace, span, flags], "-")
        end
      _ -> nil
    end
  end
  defp normalize_traceparent(_), do: nil

  defp valid_hex_id?(value, length, zero) do
    value != zero and byte_size(value) == length and Regex.match?(~r/^[0-9a-f]+$/, value)
  end

  defp random_id, do: :crypto.strong_rand_bytes(16) |> Base.encode16(case: :lower)
  defp client_ip(conn, true), do: first_req_header(conn, "x-forwarded-for") |> to_string() |> String.split(",") |> List.first() |> String.trim()
  defp client_ip(conn, false), do: conn.remote_ip |> :inet.ntoa() |> to_string()
  defp maybe_put_header(conn, _name, value) when value in [nil, ""], do: conn
  defp maybe_put_header(conn, name, value), do: put_resp_header(conn, name, value)
end
