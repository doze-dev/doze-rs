service "worker" {
  count = 2
  run   = "work --shard ${count.index}"
}
task "seed" {
  for_each = toset(["a", "b"])
  run      = "seed ${each.key}"
}
service "mail" {
  enabled = false
  run     = "mailpit"
}
