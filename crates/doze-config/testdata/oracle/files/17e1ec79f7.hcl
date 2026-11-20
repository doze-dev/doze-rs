module postgres "app" {
  version = 18
  port    = 5432

  role "app" {
    password = "app"
  }
}

module valkey "cache" {
  version = 9
  port    = 6379
}
