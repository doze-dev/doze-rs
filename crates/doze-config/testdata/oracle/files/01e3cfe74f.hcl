app "orders" {
  run  = "go run ./services/orders"
  port = 8003
  env = {
    DATABASE_URL  = module.postgres.url
    KAFKA_BROKERS = module.kafka.address
    CATALOG_URL   = app.catalog.url
  }
}
