app "orders" {
  run   = "go run ./services/orders"
  after = [task.migrate-orders]
}
