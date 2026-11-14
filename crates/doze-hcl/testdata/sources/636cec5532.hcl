app "worker" {
  run   = "go run ./cmd/worker"
  after = [task.migrate, lazy(module.cloud)]
  env   = { AWS_ENDPOINT_URL = module.cloud.url }
}
