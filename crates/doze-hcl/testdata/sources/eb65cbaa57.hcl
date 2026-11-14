module postgres "app" {
  version = 18

  role "app" {
    password = "app"
    login    = true
  }

  extension "vector" {}
}
