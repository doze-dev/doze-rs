app "worker" {
  run = "go run ./cmd/worker"
  env = { TEMPORAL_ADDRESS = module.flows.address }
}
