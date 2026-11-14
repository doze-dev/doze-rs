module mariadb "shop" {
  version = "11.4"
  port    = %d

  user "app" {
    password = "secret"
    host     = "%%"
  }
  grant {
    user       = "app"
    privileges = ["ALL PRIVILEGES"]
    database   = "shop"
  }
}
