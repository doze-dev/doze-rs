module kvrocks "store" {
  version  = 2
  password = "admin"

  namespace "tenant_a" { token = "tenant-a-token" }
}
