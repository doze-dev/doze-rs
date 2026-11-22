// What Connect shows: the same call (list the queues) from each client, pointed at this doze-aws, and the one setting S3 needs
// on an address that is an IP and port. Every value comes from `/api/v1/about`; nothing is assumed about the person's app.

export type Creds = { key: string; secret: string } | null;

export type Snippet = { id: string; title: string; install: string; code: string; s3: string };

/** The keys a snippet carries: under `--auth trust` any non-empty pair works, so `test`; under verify the configured ones,
 * which this console does not show (`null`: the snippets read them from the environment). */
export function credsFor(auth: string): Creds {
  return auth === "trust" ? { key: "test", secret: "test" } : null;
}

/** The environment block: what every SDK and the AWS CLI read. */
export function envBlock(endpoint: string, region: string, creds: Creds): string {
  const lines = [`export AWS_ENDPOINT_URL=${endpoint}`, `export AWS_REGION=${region}`];
  if (creds) lines.push(`export AWS_ACCESS_KEY_ID=${creds.key}`, `export AWS_SECRET_ACCESS_KEY=${creds.secret}`);
  return lines.join("\n");
}

/** The six clients, each listing the queues. */
export function snippets(endpoint: string, region: string, creds: Creds): Snippet[] {
  const py = creds ? `\n    aws_access_key_id="${creds.key}",\n    aws_secret_access_key="${creds.secret}",` : "";
  const js = creds ? `\n  credentials: { accessKeyId: "${creds.key}", secretAccessKey: "${creds.secret}" },` : "";
  const go = creds ? `\n\t\tconfig.WithCredentialsProvider(credentials.NewStaticCredentialsProvider("${creds.key}", "${creds.secret}", "")),` : "";
  const java = creds ? `\n    .credentialsProvider(StaticCredentialsProvider.create(AwsBasicCredentials.create("${creds.key}", "${creds.secret}")))` : "";
  return [
    {
      id: "cli",
      title: "AWS CLI",
      install: "aws --version",
      code: `aws sqs list-queues --endpoint-url ${endpoint} --region ${region}`,
      s3: "aws configure set default.s3.addressing_style path",
    },
    {
      id: "js",
      title: "JavaScript · SDK v3",
      install: "npm i @aws-sdk/client-sqs",
      code: `import { SQSClient, ListQueuesCommand } from "@aws-sdk/client-sqs";\n\nconst sqs = new SQSClient({\n  endpoint: "${endpoint}",\n  region: "${region}",${js}\n});\nconst out = await sqs.send(new ListQueuesCommand({}));\nconsole.log(out.QueueUrls);`,
      s3: "new S3Client({ endpoint, region, forcePathStyle: true })",
    },
    {
      id: "boto3",
      title: "Python · boto3",
      install: "pip install boto3",
      code: `import boto3\n\nsqs = boto3.client(\n    "sqs",\n    endpoint_url="${endpoint}",\n    region_name="${region}",${py}\n)\nprint(sqs.list_queues().get("QueueUrls"))`,
      s3: 'boto3.client("s3", endpoint_url=..., config=botocore.config.Config(s3={"addressing_style": "path"}))',
    },
    {
      id: "go",
      title: "Go · SDK v2",
      install: "go get github.com/aws/aws-sdk-go-v2/service/sqs",
      code: `cfg, err := config.LoadDefaultConfig(ctx,\n\t\tconfig.WithRegion("${region}"),${go}\n)\nif err != nil {\n\treturn err\n}\nsqsc := sqs.NewFromConfig(cfg, func(o *sqs.Options) {\n\to.BaseEndpoint = aws.String("${endpoint}")\n})\nout, err := sqsc.ListQueues(ctx, &sqs.ListQueuesInput{})`,
      s3: "s3.NewFromConfig(cfg, func(o *s3.Options) { o.UsePathStyle = true })",
    },
    {
      id: "java",
      title: "Java · SDK v2",
      install: "software.amazon.awssdk:sqs",
      code: `SqsClient sqs = SqsClient.builder()\n    .endpointOverride(URI.create("${endpoint}"))\n    .region(Region.of("${region}"))${java}\n    .build();\nSystem.out.println(sqs.listQueues().queueUrls());`,
      s3: "S3Client.builder().forcePathStyle(true)",
    },
    {
      id: "rust",
      title: "Rust · aws-sdk-sqs",
      install: "cargo add aws-config aws-sdk-sqs tokio",
      code: `let config = aws_config::defaults(aws_config::BehaviorVersion::latest())\n    .endpoint_url("${endpoint}")\n    .region(aws_config::Region::new("${region}"))\n    .load()\n    .await;\nlet sqs = aws_sdk_sqs::Client::new(&config);\nlet queues = sqs.list_queues().send().await?;`,
      s3: "aws_sdk_s3::config::Builder::from(&config).force_path_style(true)",
    },
  ];
}
