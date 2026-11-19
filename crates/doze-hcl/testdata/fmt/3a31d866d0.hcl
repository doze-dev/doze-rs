# doze.hcl — your whole local backend, declared
module postgres "app" { version = 16 }
module valkey "cache" { version = 9 }
module aws "cloud" {
  port = 4566
  bucket "uploads" {}
}
