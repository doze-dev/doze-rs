
module fake "a" { version = 1 }
module fake "c" { version = 1 }
module fake "b" {
  version = 1
  after   = [started(module.a), lazy(module.c)]
}
