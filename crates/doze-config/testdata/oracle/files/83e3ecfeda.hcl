
variable "color" { default = "teal" }
module fake "a" {
  version = 1
  color   = var.color
}
