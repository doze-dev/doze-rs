app "catalog" {
  run  = "go run ./services/catalog"
  port = 8002
  env = {
    AWS_ENDPOINT_URL = module.aws.url
    IMAGES_BUCKET    = "product-images"
  }
}
