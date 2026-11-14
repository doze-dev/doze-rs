app "catalog" {
  run   = "go run ./services/catalog"
  port  = 8002
  ready = http("/healthz")

  env = {
    DATABASE_URL     = module.postgres.url
    AWS_ENDPOINT_URL = module.aws.url
    IMAGES_BUCKET    = "product-images"
  }
}
