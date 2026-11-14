module ferret "shop" {
  version = "2.7"
  port    = %d

  database "catalog" {
    collection "products" { seed = "./products.json" }
  }
}
