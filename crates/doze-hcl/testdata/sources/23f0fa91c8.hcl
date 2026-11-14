service "mongo" {
  run  = "mongod"
  port = 27017
}
app "api" {
  run = "x"
  env = {
    ADDR = service.mongo.address
    HOST = service.mongo.host
    PORT = service.mongo.port
    DATA = service.mongo.data
  }
}
