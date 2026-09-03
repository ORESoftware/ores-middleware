defmodule OresMiddleware.Config do
  @moduledoc false

  @contract_version "1.0.0"
  @capabilities ~w(
    request-context panic-recovery request-id trace-context structured-logging
    metrics-red deadline-timeout payload-limit rate-limit auth sync-observer json
    headers compression tls-policy security-headers idempotency ip-policy cache-etag
    content-negotiation fault-injection test-auth-bypass schema-capture
  )

  def contract_version, do: @contract_version
  def capabilities, do: @capabilities

  def default(service_name) when is_binary(service_name) and byte_size(service_name) > 0 do
    %{
      contractVersion: @contract_version,
      environment: :development,
      requiredCapabilities: @capabilities,
      settings: %{
        requestIdHeader: "x-request-id",
        traceHeader: "traceparent",
        timeoutMs: 5_000,
        maxBodyBytes: 2 * 1024 * 1024,
        contextRegistryMaxEntries: 10_000,
        contextRegistryTtlMs: 30_000,
        rateLimit: %{enabled: true, capacity: 100, refillPerSecond: 20.0, keyBy: [:tenant, :user, :ip, :route]},
        compression: %{enabled: true, minimumBytes: 1_024, algorithms: ["gzip"]},
        tls: %{mode: :trusted_proxy, requireHttps: true, strictForwardedHeaders: true, trustedProxyCidrs: ["127.0.0.1/32", "::1/128"]},
        securityHeaders: %{enabled: true, hstsMaxAgeSeconds: 31_536_000, contentSecurityPolicy: "default-src 'self'; frame-ancestors 'none'", frameOptions: "DENY"},
        idempotency: %{enabled: true, headerName: "idempotency-key", ttlSeconds: 86_400, requiredMethods: ["POST", "PUT", "PATCH"]},
        faultInjection: %{enabled: false, latencyMs: 0, errorRate: 0.0, dropRate: 0.0},
        testAuthBypass: %{enabled: false, headerName: "x-test-auth-bypass", allowedCidrs: ["127.0.0.1/32", "::1/128"]},
        contentRepresentations: ["application/json", "application/problem+json"]
      },
      integrations: %{
        sharedAuth: %{mode: :disabled, issuer: nil, audience: nil, jwksUri: nil, introspectionUrl: nil, failOpen: false},
        optoSync: %{mode: :disabled, endpoint: nil, outboxTopic: nil, failOpen: true},
        oresOtel: %{enabled: true, serviceName: service_name, exporterEndpoint: nil, propagators: ["tracecontext", "baggage"]}
      }
    }
  end

  def validate(config) when is_map(config) do
    []
    |> add(config[:contractVersion] != @contract_version, "/contractVersion", "unsupported_version", "expected #{@contract_version}")
    |> add(not positive_integer?(get_in(config, [:settings, :timeoutMs])), "/settings/timeoutMs", "range", "timeout must be positive")
    |> add(not positive_integer?(get_in(config, [:settings, :maxBodyBytes])), "/settings/maxBodyBytes", "range", "body limit must be positive")
    |> add(invalid_rate_limit?(config), "/settings/rateLimit", "invalid_rate_limit", "enabled token bucket requires positive capacity and refill")
    |> add(invalid_fault_rates?(config), "/settings/faultInjection", "range", "fault rates must be within 0..=1")
    |> add(config[:environment] == :production and get_in(config, [:settings, :faultInjection, :enabled]) == true, "/settings/faultInjection/enabled", "production_forbidden", "fault injection is forbidden in production")
    |> add(config[:environment] == :production and get_in(config, [:settings, :testAuthBypass, :enabled]) == true, "/settings/testAuthBypass/enabled", "production_forbidden", "test auth bypass is forbidden in production")
    |> add(get_in(config, [:integrations, :sharedAuth, :failOpen]) == true, "/integrations/sharedAuth/failOpen", "auth_fail_open", "shared-auth must fail closed")
    |> add(get_in(config, [:settings, :tls, :mode]) == :trusted_proxy and get_in(config, [:settings, :tls, :trustedProxyCidrs]) == [], "/settings/tls/trustedProxyCidrs", "trusted_proxy_required", "trusted-proxy mode requires explicit CIDRs")
    |> add_unknown_capabilities(config[:requiredCapabilities] || [])
    |> Enum.reverse()
  end

  def validate(_), do: [%{path: "/", code: "type", message: "configuration must be a map"}]

  defp add(issues, false, _path, _code, _message), do: issues
  defp add(issues, true, path, code, message), do: [%{path: path, code: code, message: message} | issues]
  defp positive_integer?(value), do: is_integer(value) and value > 0

  defp invalid_rate_limit?(config) do
    policy = get_in(config, [:settings, :rateLimit]) || %{}
    policy[:enabled] == true and (not positive_integer?(policy[:capacity]) or not is_number(policy[:refillPerSecond]) or policy[:refillPerSecond] <= 0)
  end

  defp invalid_fault_rates?(config) do
    policy = get_in(config, [:settings, :faultInjection]) || %{}
    Enum.any?([policy[:errorRate], policy[:dropRate]], fn value -> not is_number(value) or value < 0 or value > 1 end)
  end

  defp add_unknown_capabilities(issues, capabilities) do
    Enum.reduce(capabilities, issues, fn capability, acc ->
      if capability in @capabilities, do: acc, else: [%{path: "/requiredCapabilities", code: "unknown_capability", message: to_string(capability)} | acc]
    end)
  end
end
