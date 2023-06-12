defmodule TestServer.Socket do
  @moduledoc false
  use Phoenix.Socket

  # List of exposed channels
  channel("channel:*", TestServer.Channel)

  def connect(%{"shared_secret" => "supersecret", "id" => id}, socket, _connect_info) do
    {:ok, assign(socket, :id, id)}
  end
  def connect(_, _, _), do: :error

  def id(%Phoenix.Socket{assigns: %{id: id}}), do: "sockets:#{id}"
end
