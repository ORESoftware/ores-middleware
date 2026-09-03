defmodule OresMiddlewareTest do
  use ExUnit.Case, async: false
  use Plug.Test

  test "descriptor exports the standard semantic operations" do
    descriptor = OresMiddleware.descriptor()
    assert length(descriptor.capabilities) == 23
    assert map_size(descriptor.operationSymbols) == 7
  end

  test "production rejects test-only middleware" do
    config = OresMiddleware.default_config("test")
    config = put_in(config, [:environment], :production)
    config = put_in(config, [:settings, :faultInjection, :enabled], true)
    config = put_in(config, [:settings, :testAuthBypass, :enabled], true)
    issues = OresMiddleware.validate_config(config)
    assert Enum.any?(issues, &String.contains?(&1.path, "faultInjection"))
    assert Enum.any?(issues, &String.contains?(&1.path, "testAuthBypass"))
  end

  test "Plug adapter establishes context and correlation headers" do
    config = OresMiddleware.default_config("test")
    config = put_in(config, [:settings, :tls, :requireHttps], false)
    config = put_in(config, [:settings, :rateLimit, :enabled], false)
    stack = OresMiddleware.Stack.new!(config)
    conn = conn(:get, "/v1") |> put_req_header("accept", "application/json")
    conn = OresMiddleware.Plug.wrap(stack, conn, fn conn -> Plug.Conn.resp(conn, 200, Jason.encode!(%{ok: true})) end)
    assert conn.status == 200
    assert Plug.Conn.get_resp_header(conn, "x-request-id") != []
    assert Plug.Conn.get_resp_header(conn, "traceparent") != []
    assert Plug.Conn.get_resp_header(conn, "x-content-type-options") == ["nosniff"]
  end

  test "request context is process scoped and restored" do
    context = %{request_id: "r1", trace_id: String.duplicate("a", 32), tenant_id: nil, user_id: nil}
    assert OresMiddleware.current_context() == nil
    assert OresMiddleware.run_with_context(context, fn -> OresMiddleware.current_context().request_id end) == "r1"
    assert OresMiddleware.current_context() == nil
  end
end
