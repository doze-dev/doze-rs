app "api" {
  run   = "go run ./cmd/api"
  port  = 8080
  ready = http("/healthz")
}
