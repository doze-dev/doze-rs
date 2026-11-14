task "migrate" {
  run    = "bun x prisma migrate deploy"
  inputs = ["prisma/migrations/**/*.sql"]
  env    = { DATABASE_URL = module.db.url }
}

app "api" {
  run   = "bun run dev"
  port  = 8080
  after = [task.migrate]
  env   = { DATABASE_URL = module.db.url }
}
