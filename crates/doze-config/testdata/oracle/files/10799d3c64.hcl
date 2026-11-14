
stack = "s"
app "api" {
  run = "go run ./cmd/api -v"
  env = { A = "1", B = "2" }
}
app "web" {
  run = "go run ./cmd/web"
}
