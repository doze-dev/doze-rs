app "emailer" {
  run = "go run ./cmd/emailer"
  env = { KAFKA_BROKERS = module.events.address }
}
