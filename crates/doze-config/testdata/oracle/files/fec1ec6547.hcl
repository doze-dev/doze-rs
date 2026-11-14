
module fake "db" {
  version = 1
  port    = 5432
}
module fakeproc "api" {
  port = 9090
  env  = { DATABASE_URL = module.db.url }
}
