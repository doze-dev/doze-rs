
module tleaf "base" { version = 1 }
module tdep "app" {
  version = 1
  base    = module.base.name
}
