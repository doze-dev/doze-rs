defaults {
  env = { LOG_LEVEL = "debug" }
}

app "api" {
  run      = "go run ./cmd/api"
  port     = 8080
  env_file = ".env.local"
  env      = { DATABASE_URL = module.db.url }
}
