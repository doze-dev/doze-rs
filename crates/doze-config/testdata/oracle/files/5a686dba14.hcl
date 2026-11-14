
variable "env" { default = "dev" }
variable "n" {
  type    = number
  default = 1
}
module fake "a" {
  version = 1
  port    = 7001
  color   = "${var.env}-${var.n}"
}
