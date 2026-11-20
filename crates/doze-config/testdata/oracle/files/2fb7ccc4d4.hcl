module mariadb "app" {
  version       = "11.4"
  character_set = "utf8mb4"

  user "app" {
    password = "secret"
    host     = "%"
  }

  grant {
    user       = "app"
    privileges = ["ALL PRIVILEGES"]
    database   = "app"
  }
}
