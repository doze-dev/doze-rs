module kvrocks "store" {
  version  = 2
  port     = 6380
  workers  = 4
  password = "admin"

  settings = {
    rocksdb.block_cache_size = "256"
  }

  namespace "tenant_a" {
    token = "tenant-a-token"
  }

  namespace "tenant_b" {
    token = "tenant-b-token"
  }
}
