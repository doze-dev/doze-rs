
module fake "a" { version = 1 }
module fake "b" {
  version = 1
  after   = [whenever(module.a)]
}
