module aws "aws" {
  port = 4566

  bucket "product-images" {}
  bucket "receipts" { versioning = true }

  queue "pick-lists" {
    dlq          = "auto"
    max_receives = 5
  }
}
