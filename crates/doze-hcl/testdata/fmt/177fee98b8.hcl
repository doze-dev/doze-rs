app "catalog" {
  run   = "go run ./services/catalog"
  watch = ["services/catalog/**/*.go", "platform/**/*.go"]
  port  = 8002
  ready = http("/healthz")
  after = [task.migrate-catalog]

  env = {
    DATABASE_URL     = "postgres://catalog:catalog@${module.postgres.address}/postgres?sslmode=disable"
    AWS_ENDPOINT_URL = module.aws.url
    IMAGES_BUCKET    = "product-images"
  }
}
