# 0.2
module aws "aws" {
  port = 4566

  bucket "uploads" {}
  queue "emails" {
    dlq          = "auto"
    max_receives = 5
  }
  topic "signups" {
    subscribe {
      queue = "emails"
      raw   = true
    }
  }
}
