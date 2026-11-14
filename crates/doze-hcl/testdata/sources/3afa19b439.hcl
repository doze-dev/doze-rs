variable "pg_version" {
  type    = number
  default = 18
}

locals {
  db_owner = "app_${var.pg_version}"
}

module postgres "db" {
  version = var.pg_version
  owner   = local.db_owner
}
