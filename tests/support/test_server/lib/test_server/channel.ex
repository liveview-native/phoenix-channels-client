defmodule TestServer.Channel do
  @moduledoc false

  use Phoenix.Channel

  require Logger

  alias Phoenix.Socket
  alias TestServer.Presence

  intercept ~w(deauthorized)

  def join("channel:error:" <> _, payload, _socket) do
    {:error, payload}
  end

  def join("channel:presence", _, socket) do
    send(self(), :after_presence_join)

    {:ok, socket}
  end

  def handle_info(:after_presence_join, socket) do
    {:ok, _} =
      Presence.track(socket, socket.id, %{
        online_at: inspect(System.system_time(:second))
      })

    push(socket, "presence_state", Presence.list(socket))

    {:noreply, socket}
  end

  def join("channel:protected" = channel, _, %Phoenix.Socket{assigns: %{id: id}} = socket) do
    if TestServer.Authorization.authorized?(id, channel) do
      {:ok, socket}
    else
      {:error, %{reason: "unauthorized"}}
    end
  end

  def join(topic, payload, socket) do
    IO.inspect("#{topic} was joined with #{inspect(payload)}")
    {:ok, assign(socket, :payload, payload)}
  end

  def handle_in("authorize", %{"channel" => channel, "id" => id}, socket) do
    {:reply, TestServer.Authorization.authorize(id, channel), socket}
  end

  def handle_in("deauthorize", %{"channel" => channel, "id" => id}, socket) do
    {:reply, TestServer.Authorization.deauthorize(id, channel), socket}
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

  def handle_in("reply_ok", _payload, socket) do
    {:reply, :ok, socket}
  end

  def handle_in("reply_error", _payload, socket) do
    {:reply, :error, socket}
  end

  def handle_in("reply_ok_tuple", payload, socket) do
    {:reply, {:ok, payload}, socket}
  end

  def handle_in("reply_error_tuple", payload, socket) do
    {:reply, {:error, payload}, socket}
  end

  def handle_in("broadcast" = event, payload, %Socket{topic: topic} = socket) do
    TestServer.Endpoint.broadcast!(topic, event, payload)
    {:reply, :ok, socket}
  end

  def handle_in("reply_ok_join_payload", _payload, %Socket{assigns: %{payload: payload}} = socket) do
    {:reply, {:ok, payload}, socket}
  end

  def handle_in("noreply", _payload, socket) do
    {:noreply, socket}
  end

  def handle_in("socket_disconnect", _payload, %Socket{id: id} = socket) do
    TestServer.Endpoint.broadcast!(id, "disconnect", %{})

    {:noreply, socket}
  end

  def handle_in("stop", _, socket) do
    {:stop, :shutdown, :ok, socket}
  end

  def handle_in("transport_error", _payload, socket) do
    Process.exit(socket.transport_pid, :kill)

    {:noreply, socket}
  end

  def handle_out("deauthorized" = event, %{"id" => deauthorized_id}, %Socket{id: "sockets:" <> id} = socket) do
    if deauthorized_id == id do
      {:stop, {:shutdown, :unauthorized}, socket}
    else
      {:noreply, socket}
    end
  end
end
