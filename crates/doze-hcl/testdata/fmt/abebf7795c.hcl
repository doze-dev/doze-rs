module kafka "events" {
  version = 4
  port    = 9092

  auto_create_topics = true
  default_partitions = 1
  retention          = "168h"

  topic "orders" {
    partitions = 3
  }
  topic "payments" {
    partitions = 1
    config     = { "cleanup.policy" = "compact" }
  }
}
