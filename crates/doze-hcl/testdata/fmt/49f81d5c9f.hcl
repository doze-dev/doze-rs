variable "names" {
  type    = bool
  default = true
}

defaults {
  names = var.names
}
