variable "region" {
  type = string
}
variable "n" {
  type    = number
  default = 1
}
app "api" {
  run = "x ${var.region} ${var.n}"
}
