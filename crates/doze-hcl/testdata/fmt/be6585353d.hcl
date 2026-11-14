variable "needed" {}
module fake "x" {
  version = 1
  color   = var.needed
}
