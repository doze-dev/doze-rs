app "catalog" {
  run   = "go run ./services/catalog"
  port  = 8002
  watch = ["services/catalog/**/*.go", "platform/**/*.go"]
}
