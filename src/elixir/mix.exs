defmodule OresMiddleware.MixProject do
  use Mix.Project

  def project do
    [
      app: :ores_middleware,
      version: "0.1.0",
      elixir: "~> 1.18",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      description: "Cross-framework ORES middleware contract and Plug adapter",
      source_url: "https://github.com/ORESoftware/ores-middleware"
    ]
  end

  def application do
    [
      extra_applications: [:logger, :crypto, :inets, :ssl],
      mod: {OresMiddleware.Application, []}
    ]
  end

  defp deps do
    [
      {:jason, "~> 1.4"},
      {:plug, "~> 1.20"}
    ]
  end
end
