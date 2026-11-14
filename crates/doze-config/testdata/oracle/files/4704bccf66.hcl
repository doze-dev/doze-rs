
module fake "n" {
  for_each = { a = "red", b = "blue" }
  version  = 1
  color    = each.value
}
