defmodule TestServer.MixProject do
  use Mix.Project

  def project do
    [
      app: :test_server,
      version: "0.1.0",
      elixir: "~> 1.14",
      start_permanent: Mix.env() == :prod,
      deps: deps()
    ]
  end

  # Run "mix help compile.app" to learn about applications.
  def application do
    [
      extra_applications: [:logger],
      mod: {TestServer.Application, []}
    ]
  end

  # Run "mix help deps" to learn about dependencies.
  defp deps do
    [
      {:phoenix, "~> 1.6"},
      {:jason, "~> 1.2"},
      {:plug_cowboy, "~> 2.5"},
    ]
  end
end
