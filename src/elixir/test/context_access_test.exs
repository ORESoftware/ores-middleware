defmodule OresMiddleware.ContextAccessTest do
  use ExUnit.Case, async: true

  alias OresMiddleware.ContextAccess

  @context %{
    request_id: "request-42",
    trace_id: "0123456789abcdef0123456789abcdef",
    user_id: "user-42",
    tenant_id: "tenant-7",
    baggage: %{"otel.vendor" => "test"}
  }

  test "direct accessors read a supplied immutable map" do
    assert ContextAccess.request_id(@context) == "request-42"
    assert ContextAccess.trace_id(@context) == "0123456789abcdef0123456789abcdef"
    assert ContextAccess.user_id(@context) == "user-42"
    assert ContextAccess.logged_in_user_id(@context) == "user-42"
    assert ContextAccess.tenant_id(@context) == "tenant-7"
  end

  test "ambient accessors are scoped to and restored within the current process" do
    assert OresMiddleware.current_request_id() == nil
    assert OresMiddleware.current_logged_in_user_id() == nil

    values =
      OresMiddleware.run_with_context(@context, fn ->
        {
          OresMiddleware.current_request_id(),
          OresMiddleware.current_trace_id(),
          OresMiddleware.current_user_id(),
          OresMiddleware.current_logged_in_user_id(),
          OresMiddleware.current_tenant_id()
        }
      end)

    assert values == {
             "request-42",
             "0123456789abcdef0123456789abcdef",
             "user-42",
             "user-42",
             "tenant-7"
           }

    assert OresMiddleware.current_request_id() == nil
    assert OresMiddleware.current_user_id() == nil
  end
end
