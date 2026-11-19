module aws "aws" {
  queue "emails" {
    dlq          = "auto"
    max_receives = 5
  }

  queue "orders.fifo" { fifo = true }

  topic "signups" {
    subscribe {
      queue = "emails"
      raw   = true
    }
  }
}
