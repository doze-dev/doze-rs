module postgres "shop" {
  version = 18
  owner   = "shop"

  role "shop" { password = "shop" }
  role "readonly" { login = false }

  grant {
    role       = "shop"
    database   = "shop"
    privileges = ["ALL"]
  }

  grant {
    role       = "readonly"
    schema     = "public"
    objects    = "tables"
    privileges = ["SELECT"]
  }
}
