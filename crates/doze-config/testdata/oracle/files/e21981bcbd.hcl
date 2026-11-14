
module tleaf "db" { version = 1 }
module tdep "api" {
  version = 1
  base    = module.db.name
}
