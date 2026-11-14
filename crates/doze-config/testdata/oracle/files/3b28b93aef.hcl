
module postgres "db" {
  version = 17
  sleep   = "never"
}
module valkey "cache" {
  version = 9
  sleep   = "90s"
}
