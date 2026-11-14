
module fake "db" {
  version = 1
  port    = 5432
}
app "api" {
  run  = "x"
  port = 3000
  env  = { DB = module.db.address }
}
