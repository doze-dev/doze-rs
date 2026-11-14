
stack = "shop"

module postgres "db" {
  version = 17
  port    = 5432
  role "shop" {}
}

service "mongo" {
  download = "https://fastdl.mongodb.org/osx/mongodb-macos-arm64-8.0.4.tgz"
  run      = "bin/mongod --dbpath $DATA --port $PORT"
  port     = 27017
}

app "api" {
  run   = "go run ./cmd/api"
  port  = 8080
  after = [task.migrate]
  env = {
    DATABASE_URL = module.db.url
    MONGO_URL    = "mongodb://${service.mongo.address}"
  }
}

task "migrate" {
  run    = "go run ./cmd/migrate up"
  inputs = ["migrations/*.sql"]
  env    = { DATABASE_URL = module.db.url }
}
