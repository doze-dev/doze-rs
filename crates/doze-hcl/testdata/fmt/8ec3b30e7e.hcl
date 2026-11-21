module aws "aws" {
  table "sessions" {
    key = "session_id:S"
    ttl = "expires_at"
  }
}
