app "catalog" {
  run   = "go run ./services/catalog"
  port  = 8002
  ready = http("/healthz")
}
