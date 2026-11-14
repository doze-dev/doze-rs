module postgres "db" {
  version = 16
  after   = [
    task.migrate,
  ]
  sleep   = "never"
  role "app" {}
}
