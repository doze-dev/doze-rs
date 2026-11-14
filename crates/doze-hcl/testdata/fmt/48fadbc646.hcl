module postgres "shop" {
  version = 18
  owner   = "shop"

  role "shop" {
    password         = "shop"
    connection_limit = 20
  }

  role "readonly" { login = false }

  role "analyst" {
    password  = "analyst"
    member_of = ["readonly"]
  }

  role "admin" {
    password   = "admin"
    superuser  = true
    createdb   = true
    createrole = true
  }
}
