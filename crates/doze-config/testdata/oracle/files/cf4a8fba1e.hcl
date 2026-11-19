
# a comment, and the env keys in another order
stack = "s"

app "web" {
  run = "go run ./cmd/web"
}
app "api" {
  env = { B = "2", A = "1" }
  run = "go run ./cmd/api"
}
