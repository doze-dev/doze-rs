app "api" {
  run = "go run ./cmd/api"
  env = merge(env_of(module.db, module.cache), { LOG = "debug" })
}
