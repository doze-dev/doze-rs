module postgres "tenant" {
  for_each = toset(["eu", "us"])
  version  = 18
}

service "worker" {
  count = 2
  run   = "bin/worker --shard ${count.index}"
}
