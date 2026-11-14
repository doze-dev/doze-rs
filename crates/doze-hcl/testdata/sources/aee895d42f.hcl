app "api" {
  run  = "go run ./cmd/api"
  port = 8080
  env  = { REDIS_URL = module.cache.url }
}
