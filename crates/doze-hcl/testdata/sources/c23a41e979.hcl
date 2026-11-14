module aws "aws" {
  key "app" {}
  parameter "/app/feature/banner" { value = "on" }
  secret "db-password" { value = "local-only" }
}
