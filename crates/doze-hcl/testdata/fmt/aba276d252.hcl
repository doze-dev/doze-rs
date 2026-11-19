locals {
  a = local.b
  b = local.a
}
module fake "x" { version = 1 }
