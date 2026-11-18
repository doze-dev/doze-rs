app "api" {
  run  = "go run ./cmd/api"
  port = 8080

  restart {
    policy      = "always"
    max_retries = 10
  }
}
