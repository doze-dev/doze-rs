# 0.1
s3 "media" {
  bucket "uploads" {}
}
sqs "jobs" {
  queue "emails" {}
}
sns "events" {
  sqs = sqs.jobs.name
  topic "signups" {}
  subscribe "signups" {
    protocol = "sqs"
    endpoint = "emails"
    raw      = true
  }
}
