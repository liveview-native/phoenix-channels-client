defmodule TestServer.Channel do
  @moduledoc false
  use Phoenix.Channel

  def join(topic, payload, socket) do
    IO.inspect("#{topic} was joined with #{inspect(payload)}")
    {:ok, socket}
  end

  def handle_in("send_all", payload, socket) do
    TestServer.Endpoint.broadcast!("channel:mytopic", "send_all", payload)
    {:noreply, socket}
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

  def handle_in(_event, _payload, socket) do
    {:noreply, socket}
  end
end
