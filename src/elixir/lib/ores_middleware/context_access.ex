defmodule OresMiddleware.ContextAccess do
  @moduledoc "Fast field access for the current request-scoped middleware context."

  alias OresMiddleware.Context

  @spec request_id(map()) :: term() | nil
  def request_id(context) when is_map(context), do: Map.get(context, :request_id)

  @spec trace_id(map()) :: term() | nil
  def trace_id(context) when is_map(context), do: Map.get(context, :trace_id)

  @spec user_id(map()) :: term() | nil
  def user_id(context) when is_map(context), do: Map.get(context, :user_id)

  @spec logged_in_user_id(map()) :: term() | nil
  def logged_in_user_id(context) when is_map(context), do: user_id(context)

  @spec tenant_id(map()) :: term() | nil
  def tenant_id(context) when is_map(context), do: Map.get(context, :tenant_id)

  @spec current_request_id() :: term() | nil
  def current_request_id, do: current_value(:request_id)

  @spec current_trace_id() :: term() | nil
  def current_trace_id, do: current_value(:trace_id)

  @spec current_user_id() :: term() | nil
  def current_user_id, do: current_value(:user_id)

  @spec current_logged_in_user_id() :: term() | nil
  def current_logged_in_user_id, do: current_user_id()

  @spec current_tenant_id() :: term() | nil
  def current_tenant_id, do: current_value(:tenant_id)

  defp current_value(key) do
    case Context.current() do
      context when is_map(context) -> Map.get(context, key)
      _ -> nil
    end
  end
end
