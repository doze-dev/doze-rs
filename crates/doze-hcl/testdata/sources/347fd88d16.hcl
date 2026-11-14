module postgres "app" {
  version = 18
}

app "api" {
  run  = "go run ./cmd/api"
  port = 8080
  env = {
    DATABASE_URL = module.app.url
  }
}
