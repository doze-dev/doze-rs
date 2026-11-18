app "orders" {
  run   = "go run ./services/orders"
  port  = 8003
  sleep = "never" # it consumes Kafka: no request would ever wake it
}
