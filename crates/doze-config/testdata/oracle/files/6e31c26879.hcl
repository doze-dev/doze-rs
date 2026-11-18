module valkey "cache" {
  version          = 9
  port             = 6379
  maxmemory        = "256mb"
  maxmemory_policy = "allkeys-lru"
  appendonly       = true
  password         = "cache"
  save             = "900 1 300 10"

  settings = {
    tcp-keepalive = "60"
  }
}
