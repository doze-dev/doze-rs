// The snippets "Or from your app" shows: what an empty place says it holds and how an app makes the first one, with the call written out for the
// address this doze-aws is at. Pure: the component that draws it is `from-your-app.tsx`.
export type Snippet = { cli: string; py: string };

/** The AWS CLI line and the boto3 call that make the first resource of a service, for `endpoint`. */
export function snippetFor(service: string, endpoint: string, region: string): Snippet | null {
  const e = `--endpoint-url ${endpoint}`;
  const client = (name: string) => `import boto3\nc = boto3.client("${name}", endpoint_url="${endpoint}", region_name="${region}")\n`;
  switch (service) {
    case "sqs":
      return { cli: `aws sqs create-queue --queue-name pick-lists ${e}`, py: `${client("sqs")}c.create_queue(QueueName="pick-lists")` };
    case "sns":
      return { cli: `aws sns create-topic --name orders-created ${e}`, py: `${client("sns")}c.create_topic(Name="orders-created")` };
    case "s3":
      return { cli: `aws s3api create-bucket --bucket product-images ${e}`, py: `${client("s3")}c.create_bucket(Bucket="product-images")` };
    case "dynamodb":
      return { cli: `aws dynamodb create-table --table-name orders-table \\\n  --attribute-definitions AttributeName=id,AttributeType=S \\\n  --key-schema AttributeName=id,KeyType=HASH \\\n  --billing-mode PAY_PER_REQUEST ${e}`, py: `${client("dynamodb")}c.create_table(TableName="orders-table", KeySchema=[{"AttributeName": "id", "KeyType": "HASH"}],\n    AttributeDefinitions=[{"AttributeName": "id", "AttributeType": "S"}], BillingMode="PAY_PER_REQUEST")` };
    case "kinesis":
      return { cli: `aws kinesis create-stream --stream-name events --shard-count 1 ${e}`, py: `${client("kinesis")}c.create_stream(StreamName="events", ShardCount=1)` };
    case "lambda":
      return { cli: `aws lambda create-function --function-name resize-product-image \\\n  --runtime python3.12 --handler index.handler \\\n  --role arn:aws:iam::<account>:role/<role> --zip-file fileb://fn.zip ${e}`, py: `${client("lambda")}c.create_function(FunctionName="resize-product-image", Runtime="python3.12",\n    Handler="index.handler", Role="arn:aws:iam::<account>:role/<role>", Code={"ZipFile": open("fn.zip", "rb").read()})` };
    case "logs":
      return { cli: `aws logs create-log-group --log-group-name /aws/larder ${e}`, py: `${client("logs")}c.create_log_group(logGroupName="/aws/larder")` };
    case "secretsmanager":
      return { cli: `aws secretsmanager create-secret --name larder/db-password \\\n  --secret-string 's3cret' ${e}`, py: `${client("secretsmanager")}c.create_secret(Name="larder/db-password", SecretString="s3cret")` };
    default:
      return null;
  }
}
