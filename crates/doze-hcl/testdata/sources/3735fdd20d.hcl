app "orders" {
  run = "go run ./services/orders"

  restart {
    max_retries = 10
    backoff     = "2s"
  }
}
