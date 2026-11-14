app "notifications" {
  dir   = "services/notifications"
  run   = "bun src/index.ts"
  ready = log("consuming")
  after = [task.install-notifications]

  env = {
    KAFKA_BROKERS = module.kafka.address
    SMTP_ADDR     = service.mailpit.address
  }
}
