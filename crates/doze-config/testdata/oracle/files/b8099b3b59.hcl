
app "db" {
  run = "sleep 60"
}
app "api" {
  run   = "sleep 60"
  after = [app.db]
}
app "worker" {
  run   = "sleep 60"
  after = [app.db]
}
app "mail" {
  run = "sleep 60"
}
