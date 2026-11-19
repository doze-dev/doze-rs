stack = "mods"

defaults {
  names = true
}

module kafka "events" {
  version = 4
  port    = %d
}

module aws "aws" {
  port = %d
}
