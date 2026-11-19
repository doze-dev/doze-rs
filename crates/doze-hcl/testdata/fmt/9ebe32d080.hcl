module kafka "events" {
  version            = 4
  auto_create_topics = false

  topic "orders" { partitions = 3 }
  topic "payments" {
    config = { "cleanup.policy" = "compact" }
  }
}
