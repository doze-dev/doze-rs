stack = "shop"

module postgres "db" {
  version = 16
  port    = 5432
}

module valkey "cache" {
  version = 9
  port    = 6379
}

module kafka "events" {
  version = 4
  port    = 9092
}

app "api" {
  run  = "go run ./cmd/api"
  port = 8080
  env  = { DATABASE_URL = module.db.url }
}

app "worker" {
  run = "go run ./cmd/worker"
  env = { KAFKA = module.events.address }
}

app "web" {
  run  = "go run ./cmd/web"
  port = 8090
}
