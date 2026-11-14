app "catalog" {
  run   = "go run ./services/catalog"
  port  = 8002
  ready = http("/healthz")

  env = {
    DATABASE_URL = "postgres://catalog@db.dev.internal:5432/larder"
  }
}
