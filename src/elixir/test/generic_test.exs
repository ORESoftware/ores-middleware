defmodule OresMiddleware.GenericTest do
  use ExUnit.Case, async: true

  alias OresMiddleware.Generic

  test "consumer provider identity and failure values are preserved" do
    sdk = %{prefix: "v17:"}

    provider =
      Generic.provider_from(fn token ->
        if String.starts_with?(token, sdk.prefix),
          do: {:ok, String.replace_prefix(token, sdk.prefix, "")},
          else: {:error, {:bad_token, token}}
      end)

    assert provider.("v17:alice") == {:ok, "alice"}
    assert provider.("wrong") == {:error, {:bad_token, "wrong"}}
  end

  test "contextual provider is generic over request and context" do
    provider =
      Generic.contextual_provider_from(fn request, context ->
        {context.tenant, request.token}
      end)

    assert provider.(%{request: %{token: "abc"}, context: %{tenant: "t-1"}}) ==
             {"t-1", "abc"}
  end

  test "middleware order and duplicate named stages are preserved" do
    parent = self()

    stage = fn name ->
      fn next ->
        fn request ->
          send(parent, {:before, name})
          response = next.(request)
          send(parent, {:after, name})
          response
        end
      end
    end

    handler = fn request ->
      send(parent, :handler)
      request <> ":ok"
    end

    composed =
      Generic.compose_named(handler, [
        %{name: "auth", middleware: stage.("auth")},
        %{name: "auth", middleware: stage.("auth")},
        %{name: "audit", middleware: stage.("audit")}
      ])

    assert composed.("request") == "request:ok"
    assert_receive {:before, "auth"}
    assert_receive {:before, "auth"}
    assert_receive {:before, "audit"}
    assert_receive :handler
    assert_receive {:after, "audit"}
    assert_receive {:after, "auth"}
    assert_receive {:after, "auth"}
  end

  test "empty chains preserve handler identity" do
    handler = fn request -> request end
    contextual = fn request, context -> {request, context} end
    assert Generic.compose(handler, []) === handler
    assert Generic.compose_named(handler, []) === handler
    assert Generic.compose_contextual(contextual, []) === contextual
    assert Generic.compose_named_contextual(contextual, []) === contextual
  end

  test "contextual middleware preserves caller-owned context" do
    stage = fn next ->
      fn request, context -> next.(request, Map.put(context, :seen, true)) end
    end

    handler = fn request, context -> {request, context} end
    composed = Generic.compose_contextual(handler, [stage])

    assert composed.("request", %{tenant: "t-2"}) ==
             {"request", %{tenant: "t-2", seen: true}}
  end
end
