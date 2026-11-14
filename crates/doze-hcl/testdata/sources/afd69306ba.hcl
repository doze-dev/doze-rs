module ferret "shop" {
  version = "2.7"

  database "catalog" {

    collection "products" {

    seed = "./seed/products.json"

  }
    collection "orders" {}
  }
}
