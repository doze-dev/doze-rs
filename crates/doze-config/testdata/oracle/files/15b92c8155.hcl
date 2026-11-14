
module fake "node" {
  for_each = toset(["x", "y"])
  version  = 1
  color    = each.key
}
