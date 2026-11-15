// Generates the inline policy for the Identity Center permission set `doze-recorder-limited` (main account).
// bun recorder-permission-set.ts  -> writes recorder-permission-set.json (pretty) and prints the minified size.
const R = "us-east-1";
const P = "doze-rec-";
const A = "arn:aws";

const region = { StringEquals: { "aws:RequestedRegion": R } };

const policy = {
  Version: "2012-10-17",
  Statement: [
    {
      // Things the console and the harness list or describe before they touch anything. Read-only.
      Sid: "ReadOnlyListing",
      Effect: "Allow",
      Action: [
        "sqs:ListQueues", "sns:List*", "ssm:Describe*", "ssm:ListTagsForResource", "secretsmanager:ListSecrets",
        "kms:List*", "kms:Describe*", "kms:GetKeyRotationStatus", "s3:ListAllMyBuckets", "dynamodb:List*", "dynamodb:Describe*",
        "logs:Describe*", "logs:ListTagsForResource", "events:List*", "events:Describe*", "scheduler:List*",
        "lambda:List*", "lambda:GetAccountSettings", "states:List*", "kinesis:List*", "kinesis:DescribeLimits",
        "cloudwatch:List*", "cloudwatch:Describe*", "cloudwatch:GetMetricData", "cloudwatch:GetMetricStatistics",
        "cloudformation:List*", "cloudformation:Describe*", "cloudformation:GetTemplateSummary", "cloudformation:ValidateTemplate",
        "apigateway:GET", "iam:Get*", "iam:List*", "iam:SimulatePrincipalPolicy", "sts:GetCallerIdentity", "sts:GetAccessKeyInfo",
        "budgets:ViewBudget", "ce:GetCostAndUsage", "tag:GetResources",
        // Stateless checks: they evaluate what is sent and store nothing.
        "events:TestEventPattern", "logs:TestMetricFilter", "states:TestState", "iam:SimulateCustomPolicy",
        // Learned 2026-11-15: validation with no resource, an account read, and the default schedule group (it cannot carry our prefix).
        "states:ValidateStateMachineDefinition", "kinesis:DescribeAccountSettings", "scheduler:GetScheduleGroup",
        // Learned 2026-11-15: BatchGetSecretValue is authorized on "*" (each secret is then checked by GetSecretValue on its own ARN).
        "secretsmanager:BatchGetSecretValue",
        // Checked 2026-11-15 against AWS's Service Reference (servicereference.us-east-1.amazonaws.com): these are authorized on "*",
        // not on the resource they name, and the scenarios call them.
        "events:PutPermission", "events:RemovePermission", "kinesis:UpdateShardCount", "lambda:CreateEventSourceMapping",
        "states:SendTaskSuccess", "states:SendTaskFailure", "states:SendTaskHeartbeat",
        // Learned 2026-11-15 (verify-c): a missing mapping is read with GetEventSourceMapping on "*".
        "lambda:GetEventSourceMapping",
        // Added 2026-11-15 for SSM service settings: read only (an update would change the real account's Parameter Store throughput).
        "ssm:GetServiceSetting",
        // Added 2026-11-12 for the KMS and Secrets Manager scenarios written before they were recorded: no resource, nothing stored.
        "kms:GenerateRandom", "secretsmanager:GetRandomPassword", "secretsmanager:ValidateResourcePolicy",
      ],
      Resource: "*",
      Condition: region,
    },
    {
      // Added 2026-11-15: AWS's public parameters (latest AMI ids and the like) are read by CDK and CloudFormation. Read only.
      Sid: "PublicParameters",
      Effect: "Allow",
      Action: ["ssm:GetParameter", "ssm:GetParameters", "ssm:GetParametersByPath"],
      Resource: [`${A}:ssm:${R}::parameter/aws/service/*`, `${A}:ssm:${R}::parameter/aws/reference/*`],
      Condition: region,
    },
    {
      // Everything else is limited to resources whose name starts with the doze-rec- prefix.
      Sid: "PrefixedResources",
      Effect: "Allow",
      Action: [
        "sqs:*", "sns:*", "ssm:*", "secretsmanager:*", "dynamodb:*", "logs:*", "events:*", "scheduler:*",
        "lambda:*", "states:*", "kinesis:*", "cloudwatch:*", "cloudformation:*",
      ],
      Resource: [
        `${A}:sqs:${R}:*:${P}*`,
        `${A}:sns:${R}:*:${P}*`,
        `${A}:ssm:${R}:*:parameter/${P}*`,
        // Names the scenarios use to see what AWS refuses (and accepts): reserved words, a path with no slash, and the Secrets
        // Manager reference path. AWS refuses the first four before it makes anything; `/awsome-dzc/*` it may accept, and cleanup deletes it.
        `${A}:ssm:${R}:*:parameter/aws/not-yours`,
        `${A}:ssm:${R}:*:parameter/ssm-not-yours`,
        `${A}:ssm:${R}:*:parameter/awsome-dzc/*`,
        `${A}:ssm:${R}:*:parameter/no-leading-slash`,
        `${A}:ssm:${R}:*:parameter/aws/reference/secretsmanager`,
        `${A}:ssm:${R}:*:parameter/aws/reference/secretsmanager/${P}*`,
        `${A}:ssm:${R}:*:document/${P}*`,
        `${A}:secretsmanager:${R}:*:secret:${P}*`,
        `${A}:dynamodb:${R}:*:table/${P}*`,
        `${A}:dynamodb:${R}:*:table/${P}*/*`,
        `${A}:logs:${R}:*:log-group:${P}*`,
        `${A}:logs:${R}:*:log-group:${P}*:*`,
        `${A}:logs:${R}:*:log-group:/${P}*`,
        `${A}:logs:${R}:*:log-group:/${P}*:*`,
        `${A}:logs:${R}:*:log-group:/aws/lambda/${P}*`,
        `${A}:logs:${R}:*:log-group:/aws/lambda/${P}*:*`,
        `${A}:events:${R}:*:rule/${P}*`,
        `${A}:events:${R}:*:rule/${P}*/*`,
        `${A}:events:${R}:*:event-bus/${P}*`,
        `${A}:events:${R}:*:archive/${P}*`,
        `${A}:events:${R}:*:connection/${P}*`,
        `${A}:events:${R}:*:api-destination/${P}*`,
        `${A}:scheduler:${R}:*:schedule/*/${P}*`,
        `${A}:scheduler:${R}:*:schedule-group/${P}*`,
        // Learned 2026-11-15: DeleteScheduleGroup asks for scheduler:DeleteSchedule on every schedule of the group (schedule/<group>/*).
        `${A}:scheduler:${R}:*:schedule/${P}*/*`,
        // Learned 2026-11-15 (verify-c): the refusals scenarios ask to delete the default bus and the default schedule group, which AWS
        // refuses itself; without these the recorder's policy refuses first and the label records AccessDenied instead of AWS's answer.
        `${A}:events:${R}:*:event-bus/default`,
        `${A}:scheduler:${R}:*:schedule-group/default`,
        `${A}:lambda:${R}:*:function:${P}*`,
        `${A}:lambda:${R}:*:function:${P}*:*`,
        `${A}:lambda:${R}:*:layer:${P}*`,
        `${A}:lambda:${R}:*:layer:${P}*:*`,
        `${A}:lambda:${R}:*:event-source-mapping:*`,
        `${A}:states:${R}:*:stateMachine:${P}*`,
        `${A}:states:${R}:*:execution:${P}*:*`,
        `${A}:states:${R}:*:activity:${P}*`,
        // Added 2026-11-14: express executions and map runs of our state machines.
        `${A}:states:${R}:*:express:${P}*:*`,
        `${A}:states:${R}:*:mapRun:${P}*/*`,
        `${A}:kinesis:${R}:*:stream/${P}*`,
        `${A}:kinesis:${R}:*:stream/${P}*/consumer/*`,
        `${A}:cloudwatch:${R}:*:alarm:${P}*`,
        `${A}:cloudwatch::*:dashboard/${P}*`,
        // Added 2026-11-14 (gaps stream): Contributor Insights rules and metric streams are stored now.
        `${A}:cloudwatch:${R}:*:insight-rule/${P}*`,
        `${A}:cloudwatch:${R}:*:metric-stream/${P}*`,
        `${A}:cloudformation:${R}:*:stack/${P}*/*`,
        `${A}:cloudformation:${R}:*:stackset/${P}*:*`,
        // Added 2026-11-14: a SAM template's change set names AWS's own transform as a resource.
        `${A}:cloudformation:${R}:aws:transform/*`,
      ],
      Condition: region,
    },
    {
      Sid: "AccountLevelCallsThatCannotBePrefixed",
      Effect: "Allow",
      // execute-api:ManageConnections added 2026-11-14: the WebSocket @connections API (correctness stream).
      // cloudwatch:PutCompositeAlarm added 2026-11-15: AWS authorizes it against alarm:*, not the alarm's own name.
      Action: ["cloudwatch:PutMetricData", "cloudwatch:PutCompositeAlarm", "apigateway:*", "execute-api:Invoke", "execute-api:ManageConnections"],
      Resource: "*",
      Condition: region,
    },
    {
      // KMS has no names for keys, so we mark our own: keys are created with the tag doze-rec=true and only tagged keys are usable.
      Sid: "KmsCreateTaggedKeys",
      Effect: "Allow",
      Action: ["kms:CreateKey", "kms:TagResource"],
      Resource: "*",
      Condition: { StringEquals: { "aws:RequestTag/doze-rec": "true", "aws:RequestedRegion": R } },
    },
    {
      Sid: "KmsOnlyOurKeys",
      Effect: "Allow",
      Action: "kms:*",
      Resource: `${A}:kms:${R}:*:key/*`,
      Condition: { StringEquals: { "aws:ResourceTag/doze-rec": "true" } },
    },
    {
      // Use of any key, including AWS-managed ones, but only on behalf of a service (SSM SecureString, Secrets Manager, S3, ...).
      Sid: "KmsUseViaServices",
      Effect: "Allow",
      Action: ["kms:Encrypt", "kms:Decrypt", "kms:ReEncrypt*", "kms:GenerateDataKey*", "kms:DescribeKey", "kms:CreateGrant"],
      Resource: `${A}:kms:${R}:*:key/*`,
      Condition: { StringEquals: { "kms:ViaService": ["ssm", "secretsmanager", "s3", "dynamodb", "sqs", "sns", "logs", "lambda", "kinesis", "events", "states"].map((x) => `${x}.${R}.amazonaws.com`) } },
    },
    {
      Sid: "KmsOurAliases",
      Effect: "Allow",
      Action: ["kms:CreateAlias", "kms:DeleteAlias", "kms:UpdateAlias"],
      // `alias/aws/mine` is a name AWS reserves: the scenario shows that it refuses it, and nothing is made.
      Resource: [`${A}:kms:${R}:*:alias/${P}*`, `${A}:kms:${R}:*:alias/aws/mine`],
    },
    {
      Sid: "S3PrefixedBuckets",
      Effect: "Allow",
      Action: "s3:*",
      Resource: [`${A}:s3:::${P}*`, `${A}:s3:::${P}*/*`],
    },
    {
      Sid: "IamPrefixedPrincipals",
      Effect: "Allow",
      Action: "iam:*",
      // IAM ARNs carry the path before the name (user/apps/doze-rec-x), so each kind is allowed with and without a path.
      Resource: ["role", "user", "group", "policy", "instance-profile", "saml-provider"].flatMap((k) => [`${A}:iam::*:${k}/${P}*`, `${A}:iam::*:${k}/*/${P}*`]),
    },
    {
      // Added 2026-11-14: API Gateway execution logs land in groups named by the API's id, which cannot carry our prefix. Read only.
      Sid: "ReadApiGatewayExecutionLogs",
      Effect: "Allow",
      Action: ["logs:DescribeLogStreams", "logs:GetLogEvents", "logs:FilterLogEvents"],
      Resource: [`${A}:logs:${R}:*:log-group:API-Gateway-Execution-Logs_*`, `${A}:logs:${R}:*:log-group:API-Gateway-Execution-Logs_*:*`],
    },
    {
      // Added 2026-11-14: role sessions for the scenarios that act as a role (Scheduler and IAM enforce paths). Our roles only.
      Sid: "AssumeOurRoles",
      Effect: "Allow",
      Action: ["sts:AssumeRole", "sts:TagSession"],
      Resource: [`${A}:iam::*:role/${P}*`, `${A}:iam::*:role/*/${P}*`],
    },
    {
      Sid: "NeverTheseEvenIfAnotherPolicyAllowsThem",
      Effect: "Deny",
      Action: [
        "organizations:*", "account:*", "billing:*", "aws-portal:*", "payments:*", "sso:*", "sso-directory:*", "identitystore:*",
        "iam:CreateAccountAlias", "iam:DeleteAccountAlias", "iam:UpdateAccountPasswordPolicy", "iam:DeleteAccountPasswordPolicy",
        "iam:CreateLoginProfile", "iam:UpdateLoginProfile",
        "ec2:RunInstances", "ec2:CreateNatGateway", "ec2:AllocateAddress", "rds:Create*", "redshift:Create*",
        "elasticache:Create*", "es:CreateDomain", "eks:CreateCluster", "ecs:CreateService",
        "kafka:CreateCluster*", "elasticmapreduce:RunJobFlow", "sagemaker:Create*", "bedrock:*",
        "lambda:PutProvisionedConcurrencyConfig", "dynamodb:PurchaseReservedCapacityOfferings",
      ],
      Resource: "*",
    },
    {
      // Belt and braces: keys that are not ours (for example the Identity Center key) can never be disabled, deleted or re-policied.
      Sid: "NeverTouchKeysThatAreNotOurs",
      Effect: "Deny",
      Action: [
        "kms:ScheduleKeyDeletion", "kms:DisableKey", "kms:PutKeyPolicy", "kms:DeleteImportedKeyMaterial",
        "kms:RevokeGrant", "kms:RetireGrant", "kms:CreateGrant", "kms:ReplicateKey", "kms:UpdatePrimaryRegion",
        "kms:EnableKeyRotation", "kms:DisableKeyRotation",
      ],
      Resource: `${A}:kms:*:*:key/*`,
      Condition: { StringNotEquals: { "aws:ResourceTag/doze-rec": "true" } },
    },
  ],
};

const pretty = JSON.stringify(policy, null, 2);
const min = JSON.stringify(policy);
await Bun.write(import.meta.dir + "/recorder-permission-set.json", pretty + "\n");
await Bun.write(import.meta.dir + "/recorder-permission-set.min.json", min + "\n");
console.log(`pretty ${pretty.length} chars, minified ${min.length} chars`);
