defmodule TestServer.Authorization do
  use GenServer

  def authorize(id, channel) do
    GenServer.call(__MODULE__, {:authorize, id, channel})
  end

  def authorized?(id, channel) do
    GenServer.call(__MODULE__, {:authorized?, id, channel})
  end

  def deauthorize(id, channel) do
    GenServer.call(__MODULE__, {:deauthorize, id, channel})
  end

  def start_link([]) do
    GenServer.start_link(__MODULE__, [], name: __MODULE__)
  end

  @impl GenServer
  def init([]) do
    {:ok, %{}}
  end

  @impl GenServer
  def handle_call({:authorize, id, channel}, _from, id_set_by_channel) do
    {_, new_id_set_by_channel} = Map.get_and_update(id_set_by_channel, channel, fn id_set ->
      new_id_set = MapSet.put(id_set || MapSet.new(), id)
      {id_set, new_id_set}
    end)

    {:reply, :ok, new_id_set_by_channel}
  end

  def handle_call({:authorized?, id, channel}, _from, id_set_by_channel) do
    authorized? = case id_set_by_channel do
      %{^channel => id_set} -> MapSet.member?(id_set, id)
      _ -> false
    end

    {:reply, authorized?, id_set_by_channel}
  end

  def handle_call({:deauthorize, id, channel}, _from, id_set_by_channel) do
    {_, new_id_set_by_channel} = Map.get_and_update(id_set_by_channel, channel, fn id_set ->
      case id_set do
        nil -> :pop
        _ ->
          new_id_set = MapSet.delete(id_set, id)

          if Enum.empty?(new_id_set) == 0 do
            :pop
          else
            {id_set, new_id_set}
          end
      end
    end)

    TestServer.Endpoint.broadcast!(channel, "deauthorized", %{"id" => id})

    {:reply, :ok, new_id_set_by_channel}
  end
end
