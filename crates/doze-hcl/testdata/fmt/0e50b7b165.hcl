variable "tenants" {
  default = { acme = 5432, globex = 5433, initech = 5434 }
}

module postgres "tenant" {
  for_each = var.tenants
  version  = 18
  port     = each.value
  owner    = each.key
}
