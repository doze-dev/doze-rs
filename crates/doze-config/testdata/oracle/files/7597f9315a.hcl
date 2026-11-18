
module fake "node" {
  for_each = toset(["a"])
  version  = 1
  color    = "x"
}
module fake "client" {
  version = 1
  ref     = module.node_a.name
}
