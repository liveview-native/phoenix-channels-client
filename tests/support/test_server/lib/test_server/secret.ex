defmodule TestServer.Secret do
  use GenServer

  def generate(id) do
    GenServer.call(__MODULE__, {:generate, id})
  end

  def check(id, secret) do
    GenServer.call(__MODULE__, {:check, id, secret})
  end

  def delete(id, secret) do
    GenServer.call(__MODULE__, {:delete, id, secret})
  end

  def start_link([]) do
    GenServer.start_link(__MODULE__, [], name: __MODULE__)
  end

  @impl GenServer
  def init([]) do
    {:ok, %{}}
  end

  @impl GenServer
  def handle_call({:generate, id}, _from, state) do
    secret =
      20
      |> :crypto.strong_rand_bytes()
      |> Base.encode64()

    {:reply, secret, Map.put(state, id, secret)}
  end

  def handle_call({:check, id, secret}, _from, state) do
    case state do
      %{^id => ^secret} -> {:reply, :ok, state}
      _ -> {:reply, :error, state}
    end
  end

  def handle_call({:delete, id, secret}, _from, state) do
    case state do
      %{^id => ^secret} -> {:reply, :ok, Map.delete(state, id)}
      _ -> {:reply, :error, state}
    end
  end
end
