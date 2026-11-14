module aws "aws" {
  function "resize" {
    code    = "./functions/resize"
    timeout = 10
  }

  rule "order-placed" {
    pattern = "{\"source\":[\"shop.checkout\"]}"
    targets = ["lambda:resize", "queue:emails"]
  }
}
