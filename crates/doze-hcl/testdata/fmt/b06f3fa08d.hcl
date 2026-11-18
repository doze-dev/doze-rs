defaults {
  sleep = "10m"
}

module postgres "db" {
  version = 18
  sleep   = "30m"
}

app "orders" {
  run   = "go run ./services/orders"
  port  = 8003
  sleep = "never"
}
