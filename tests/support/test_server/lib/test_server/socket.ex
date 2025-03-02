defmodule TestServer.Socket do
  @moduledoc false
  use Phoenix.Socket

  # List of exposed channels
  channel("channel:*", TestServer.Channel)

  def connect(%{"shared_secret" => "supersecret", "id" => id}, socket, _connect_info) do
    intentional_failures = Application.get_env(:test_server, :socket_reconnect_failures, %{})

    if Map.get(intentional_failures, id, false) do
      updated_failures = Map.delete(intentional_failures, id)
      Application.put_env(:test_server, :socket_reconnect_failures, updated_failures)
      :error
    else
      {:ok, assign(socket, :id, id)}
    end
  end

  def connect(%{"id" => id, "secret" => secret}, socket, _connect_info) do
    case TestServer.Secret.check(id, secret) do
      :ok -> {:ok, assign(socket, %{id: id, secret: secret})}
      :error -> :error
    end
  end

  def connect(_, _, _), do: :error

  def id(%Phoenix.Socket{assigns: %{id: id}}), do: "sockets:#{id}"
end
