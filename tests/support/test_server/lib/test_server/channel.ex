defmodule TestServer.Channel do
  @moduledoc false

  use Phoenix.Channel

  require Logger

  alias Phoenix.Socket

  def join("channel:error:" <> _, payload, _socket) do
    {:error, payload}
  end

  def join(topic, payload, socket) do
    IO.inspect("#{topic} was joined with #{inspect(payload)}")
    {:ok, assign(socket, :payload, payload)}
  end

  def handle_in("generate_secret", _, %Phoenix.Socket{assigns: %{id: id}} = socket) do
    {:reply, {:ok, TestServer.Secret.generate(id)}, socket}
  end

  def handle_in("delete_secret", _, %Phoenix.Socket{assigns: %{id: id, secret: secret}} = socket) do
    case TestServer.Secret.delete(id, secret) do
      :ok ->
        Logger.info("secret #{secret} deleted for id #{id}")
        {:reply, :ok, socket}
      :error ->
        Logger.info("secret #{secret} does not match for id #{id}")
        {:reply, :error, socket}
    end
  end

  def handle_in("raise", payload, _socket) do
    raise payload
  end

  def handle_in("send_all" = event, payload, %Socket{topic: topic} = socket) do
    TestServer.Endpoint.broadcast!(topic, event, payload)
    {:reply, :ok, socket}
  end

  def handle_in("send_join_payload", _payload, %Socket{assigns: %{payload: payload}} = socket) do
    {:reply, {:ok, payload}, socket}
  end

  def handle_in("send_reply", payload, socket) do
    {:reply, {:ok, payload}, socket}
  end

  def handle_in("send_noreply", _payload, socket) do
    {:noreply, socket}
  end

  def handle_in("send_error", _payload, socket) do
    {:reply, :error, socket}
  end

  def handle_in("socket_disconnect", _payload, %Phoenix.Socket{id: id} = socket) do
    TestServer.Endpoint.broadcast!(id, "disconnect", %{})

    {:noreply, socket}
  end

  def handle_in("transport_error", _payload, socket) do
    Process.exit(socket.transport_pid, :kill)

    {:noreply, socket}
  end
end
