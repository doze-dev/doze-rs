app "api" {
  run  = "bun run dev"
  port = 3000
  env  = { MONGODB_URI = module.docs.url }
}
