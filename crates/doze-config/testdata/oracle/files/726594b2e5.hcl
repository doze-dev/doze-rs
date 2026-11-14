
module fake "a" { version = 1 }
module fake "b" {
  version = 1
  ref     = module.a.name
  after   = [started(module.a)]
}
