defmodule OresMiddleware.TraceparentPolicyTest do
  use ExUnit.Case, async: false

  import Plug.Conn
  import Plug.Test, only: [conn: 2]

  @zero_trace_id String.duplicate("0", 32)
  @valid_trace_id "0123456789abcdef0123456789abcdef"
  @valid_parent_span_id "0123456789abcdef"
  @valid_server_span_id "fedcba9876543210"

  defp stack do
    config = OresMiddleware.default_config("traceparent-policy-test")
    config = put_in(config, [:environment], :test)
    config = put_in(config, [:settings, :tls, :requireHttps], false)
    config = put_in(config, [:settings, :rateLimit, :enabled], false)
    config = put_in(config, [:settings, :idempotency, :enabled], false)
    OresMiddleware.Stack.new!(config)
  end

  defp request(trace_id) do
    conn(:get, "/trace")
    |> put_req_header("accept", "application/json")
    |> put_req_header(
      "traceparent",
      "00-#{trace_id}-#{@valid_parent_span_id}-01"
    )
  end

  test "the inbound parent is not relabelled as a response server span" do
    response =
      OresMiddleware.Plug.wrap(stack(), request(@valid_trace_id), fn conn ->
        resp(conn, 204, "")
      end)

    assert get_resp_header(response, "traceparent") == []
  end

  test "only a valid tracer-owned response traceparent is preserved" do
    valid = "00-#{@valid_trace_id}-#{@valid_server_span_id}-01"

    valid_response =
      OresMiddleware.Plug.wrap(stack(), request(@valid_trace_id), fn conn ->
        conn
        |> put_resp_header("traceparent", String.upcase(valid))
        |> resp(204, "")
      end)

    assert get_resp_header(valid_response, "traceparent") == [valid]

    invalid_response =
      OresMiddleware.Plug.wrap(stack(), request(@valid_trace_id), fn conn ->
        conn
        |> put_resp_header(
          "traceparent",
          "00-#{@valid_trace_id}-0000000000000000-01"
        )
        |> resp(204, "")
      end)

    assert get_resp_header(invalid_response, "traceparent") == []
  end

  test "an all-zero inbound trace ID is replaced" do
    response =
      OresMiddleware.Plug.wrap(stack(), request(@zero_trace_id), fn conn ->
        context = OresMiddleware.current_context()
        assert context.trace_id != @zero_trace_id
        assert context.trace_id =~ ~r/^[0-9a-f]{32}$/
        resp(conn, 204, "")
      end)

    assert response.status == 204
    assert get_resp_header(response, "traceparent") == []
  end
end
