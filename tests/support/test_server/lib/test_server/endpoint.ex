defmodule TestServer.Endpoint do
  @moduledoc false
  use Phoenix.Endpoint, otp_app: :test_server

  socket("/socket", TestServer.Socket, websocket: true, longpoll: false)

  @doc false
  def init(:supervisor, config) do
    {:ok,
     Keyword.merge(
       config,
       https: false,
       http: [port: 9002],
       check_origin: false,
       secret_key_base: String.duplicate("abcdefgh", 8),
       debug_errors: false,
       server: true
     )}
  end
end
