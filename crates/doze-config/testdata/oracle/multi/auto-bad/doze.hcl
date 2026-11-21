variable "n" {
  type = number
}
app "api" {
  run = "${var.n}"
}
