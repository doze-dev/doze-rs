
module fake "x" {
  count    = 2
  for_each = toset(["a"])
}
