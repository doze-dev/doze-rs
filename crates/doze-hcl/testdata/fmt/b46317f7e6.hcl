module aws "cloud" {
  port = 4566

  bucket "uploads" {}
  table "orders" {
    key = "id:S"
  }
}
