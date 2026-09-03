defmodule OresMiddleware do
  @moduledoc "Cross-language ORES middleware contract for Elixir/OTP servers."

  alias OresMiddleware.{Config, Context, Stack}

  def capabilities, do: Config.capabilities()
  def default_config(service_name), do: Config.default(service_name)
  def validate_config(config), do: Config.validate(config)
  def current_context, do: Context.current()
  def run_with_context(context, operation), do: Context.run(context, operation)

  def create_middleware(config, hooks \\ %{}) do
    case Stack.new(config, hooks) do
      {:ok, stack} -> {:ok, fn conn, next -> OresMiddleware.Plug.wrap(stack, conn, next) end}
      error -> error
    end
  end

  def descriptor do
    %{
      contractVersion: Config.contract_version(),
      language: "elixir",
      runtime: "beam-otp",
      packageName: "ores_middleware",
      frameworkAdapters: ["plug", "phoenix", "bandit", "cowboy", "otp"],
      capabilities: capabilities(),
      operationSymbols: %{
        descriptor: "OresMiddleware.descriptor/0",
        defaultConfig: "OresMiddleware.default_config/1",
        validateConfig: "OresMiddleware.validate_config/1",
        createMiddleware: "OresMiddleware.create_middleware/2",
        runWithContext: "OresMiddleware.run_with_context/2",
        currentContext: "OresMiddleware.current_context/0",
        capabilities: "OresMiddleware.capabilities/0"
      }
    }
  end

  def descriptor_json, do: Jason.encode!(descriptor())
end
