module aws "local" {
  port = 4566

  bucket "uploads" { versioning = true }
  queue "emails" {
    dlq          = "auto"
    max_receives = 5
  }
  queue "orders" {
    fifo          = true
    content_dedup = true
  }

  topic "signups" {

    subscribe {

      queue = "emails"

      raw = true

    }
  }

  table "sessions" {
    key = "session_id:S"
    ttl = "expires_at"
  }

  function "resize" {
    code    = "./functions/resize" # dir with a provided.al2 bootstrap
    timeout = 10
  }

  rule "order_placed" {
    pattern = "{\"source\":[\"shop.checkout\"]}"
    targets = ["lambda:resize", "queue:emails"]
  }

  key "app_key" {}
  secret "db_password" { value = "s3cr3t" }
  parameter "/app/feature/banner" { value = "hello" }

  state_machine "order_flow" {
    definition = <<-ASL
      {
        "StartAt": "Resize",
        "States": {
          "Resize": {
            "Type": "Task",
            "Resource": "arn:aws:lambda:us-east-1:000000000000:function:resize",
            "Retry": [{"ErrorEquals": ["States.TaskFailed"], "MaxAttempts": 2}],
            "Next": "Notify"
          },
          "Notify": {
            "Type": "Task",
            "Resource": "arn:aws:states:::sqs:sendMessage",
            "Parameters": {"QueueUrl": "http://localhost:4566/000000000000/emails", "MessageBody.$": "$"},
            "End": true
          }
        }
      }
    ASL
  }
}
