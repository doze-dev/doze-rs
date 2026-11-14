
module fake "x" {
  version = 1
  ref     = module.y.name
}
module fake "y" {
  version = 1
  ref     = module.x.name
}
