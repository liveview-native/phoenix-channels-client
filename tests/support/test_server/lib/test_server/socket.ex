defmodule TestServer.Socket do
  @moduledoc false
  use Phoenix.Socket

  # List of exposed channels
  channel("channel:*", TestServer.Channel)

  def connect(params, socket, _connect_info) do
    case params["shared_secret"] do
      "supersecret" -> {:ok, socket}
      _ -> :error
    end
  end

  def id(_socket), do: ""
end
