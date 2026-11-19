variable "base" { default = "red" }
locals {
  a = upper(var.base)
  b = "${local.a}-x"
}
module fake "x" {
  version = 1
  color   = local.b
}
