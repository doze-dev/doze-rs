stack = "shop"
app "api" {
  run  = "x"
  port = 8080
  env  = { DB = module.db.url }
}
