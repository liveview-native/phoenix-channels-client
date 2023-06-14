defmodule TestServer.Application do
  # See https://hexdocs.pm/elixir/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    children = [
      {Phoenix.PubSub, [adapter: Phoenix.PubSub.PG2, name: TestServer.PubSub]},
      TestServer.Endpoint,
      TestServer.Secret
      # Starts a worker by calling: TestServer.Worker.start_link(arg)
      # {TestServer.Worker, arg}
    ]

    # See https://hexdocs.pm/elixir/Supervisor.html
    # for other strategies and supported options
    opts = [strategy: :one_for_one, name: TestServer.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
