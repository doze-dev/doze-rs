
module valkey "cache" { version = 9 }
module fake "api" {
  version = 1
  port    = 7000
  ref     = module.cache.address
}
