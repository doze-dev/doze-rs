
stack = "s"
app "api" {
  run = "go run ./cmd/api"
  env = { A = "1", B = "2" }
}
app "web" {
  run = "go run ./cmd/web"
}
