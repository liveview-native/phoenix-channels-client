import Config

config :test_server, TestServer.Endpoint,
  pubsub_server: TestServer.PubSub

config :phoenix, json_library: Jason
