
module tquiet "db" {
  version = 1
  port    = 15432
}
module tquiet "api" {
  version = 1
  port    = 18080
  after   = [module.db]
}
