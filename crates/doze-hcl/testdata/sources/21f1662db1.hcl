app "api" {
  run  = "php artisan serve --port=$PORT"
  port = 8000
  env  = { DATABASE_URL = "mysql://app:secret@${module.app.address}/app" }
}
