service "mongo" {
  run  = "mongod --dbpath $DATA --bind_ip $HOST --port $PORT"
  port = 27017
}
