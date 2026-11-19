app "api" {
  run = "go run ./cmd/api"
  env = { STRIPE_KEY = env("STRIPE_KEY", "sk_test_local") }
}
