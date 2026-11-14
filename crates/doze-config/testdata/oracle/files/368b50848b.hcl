stack = "shop"

module postgres "db" {
  version = 16 # keep
  role "shop" {}
}

app "api" {
  run = "go run ./cmd/api"
  env = { DATABASE_URL = module.db.url }
}

module kafka "events" {
  version = 4
}
