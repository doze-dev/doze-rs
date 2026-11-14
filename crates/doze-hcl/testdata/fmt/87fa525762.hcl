service "mongo" {
  # a container: a container is just a command, and `run` stays in the foreground
  run  = "docker run --rm -p $HOST:$PORT:27017 mongo:7"
  port = 27017
}
