// Generates the inline policy for the Identity Center permission set `doze-recorder-limited` (main account).
// bun recorder-permission-set.ts  -> writes recorder-permission-set.json (pretty) and prints the minified size.
// Two regions: us-east-1 for everything, us-west-2 for what is about a second region (Secrets Manager replicas, multi-Region KMS
// keys). ARNs take any region; the condition holds every call to these two.
const REGIONS = ["us-east-1", "us-west-2"];
const R = "*";
const P = "doze-rec-";
const A = "arn:aws";

const region = { StringEquals: { "aws:RequestedRegion": REGIONS } };

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
        // Added 2026-11-15 from the roadmap audit (every operation built or planned, checked against the Service Reference): these
        // are authorized on "*" and change nothing outside what the scenarios make and delete.
        "cloudformation:EstimateTemplateCost", "cloudformation:GetHookResult", "cloudformation:ListHookResults",
        "cloudwatch:PutAnomalyDetector", "cloudwatch:DeleteAnomalyDetector", "iam:GenerateCredentialReport",
        "kinesis:EnableEnhancedMonitoring", "kinesis:DisableEnhancedMonitoring", "kinesis:UpdateStreamMode",
        "lambda:CreateCodeSigningConfig", "logs:CancelExportTask", "logs:PutIndexPolicy", "logs:DeleteIndexPolicy",
        "logs:PutQueryDefinition", "logs:DeleteQueryDefinition", "logs:PutResourcePolicy", "logs:DeleteResourcePolicy",
        "logs:GetLogFields", "logs:ListLogGroups", "logs:ListLogGroupsForQuery", "logs:ListScheduledQueries", "logs:StopQuery",
        "logs:TestTransformer", "logs:StartQuery", "logs:GetQueryResults", "logs:StartLiveTail", "logs:CreateExportTask",
        "sns:CheckIfPhoneNumberIsOptedOut", "sns:GetSMSAttributes", "sns:CreatePlatformApplication", "sns:DeletePlatformApplication",
        "sns:GetPlatformApplicationAttributes", "sns:SetPlatformApplicationAttributes", "sns:CreatePlatformEndpoint", "sns:DeleteEndpoint",
        "sns:GetEndpointAttributes", "sns:SetEndpointAttributes", "ssm:ListDocuments", "states:RevealSecrets",
        "sts:DecodeAuthorizationMessage", "sts:GetSessionToken",
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
        "lambda:*", "states:*", "kinesis:*", "cloudwatch:*", "cloudformation:*", "s3:*", "iam:*",
      ],
      Resource: [
        // Any resource of these services whose name has our prefix, in any of the two regions (ARN segments: service, region,
        // account, resource; a dashboard has no region, a bucket neither region nor account, and `*` matches those empty).
        `${A}:*:*:*:*${P}*`,
        // Names the scenarios use to see what AWS refuses (and accepts): reserved words, a path with no slash, and the Secrets
        // Manager reference path. AWS refuses the first four before it makes anything; `/awsome-dzc/*` it may accept, and cleanup deletes it.
        `${A}:ssm:*:*:parameter/aws/not-yours`, `${A}:ssm:*:*:parameter/ssm-not-yours`, `${A}:ssm:*:*:parameter/awsome-dzc/*`,
        `${A}:ssm:*:*:parameter/no-leading-slash`, `${A}:ssm:*:*:parameter/aws/reference/secretsmanager`,
        // The refusals scenarios ask to delete the default bus and the default schedule group, which AWS refuses itself.
        `${A}:events:*:*:event-bus/default`, `${A}:scheduler:*:*:schedule-group/default`,
        // Named by AWS (an id), not by us.
        `${A}:logs:*:*:scheduled-query:*`, `${A}:lambda:*:*:code-signing-config:*`, `${A}:lambda:*:*:event-source-mapping:*`,
        // A SAM template's change set names AWS's own transform as a resource.
        `${A}:cloudformation:*:aws:transform/*`,
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
      Condition: { StringEquals: { "aws:RequestTag/doze-rec": "true", "aws:RequestedRegion": REGIONS } },
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
      Condition: { StringLike: { "kms:ViaService": ["ssm", "secretsmanager", "s3", "dynamodb", "sqs", "sns", "logs", "lambda", "kinesis", "events", "states"].map((x) => `${x}.*.amazonaws.com`) }, StringEquals: { "aws:RequestedRegion": REGIONS } },
    },
    {
      Sid: "KmsOurAliases",
      Effect: "Allow",
      Action: ["kms:CreateAlias", "kms:DeleteAlias", "kms:UpdateAlias"],
      // `alias/aws/mine` is a name AWS reserves: the scenario shows that it refuses it, and nothing is made.
      Resource: [`${A}:kms:${R}:*:alias/${P}*`, `${A}:kms:${R}:*:alias/aws/mine`],
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
      Action: ["sts:AssumeRole", "sts:TagSession", "sts:SetSourceIdentity", "sts:SetContext", "sts:GetFederationToken"],
      Resource: [`${A}:iam::*:role/${P}*`, `${A}:iam::*:role/*/${P}*`, `${A}:sts::*:federated-user/${P}*`],
    },
    {
      // Added 2026-11-15: two scenarios need a service-linked role AWS makes on first use (DynamoDB Contributor Insights, API Gateway
      // tracing). Only these two services, so the recorder never makes another.
      Sid: "TheServiceLinkedRolesTheScenariosNeed",
      Effect: "Allow",
      Action: "iam:CreateServiceLinkedRole",
      Resource: "*",
      Condition: { StringEquals: { "iam:AWSServiceName": ["contributorinsights.dynamodb.amazonaws.com", "ops.apigateway.amazonaws.com"] } },
    },
    {
      Sid: "NeverTheseEvenIfAnotherPolicyAllowsThem",
      Effect: "Deny",
      Action: [
        "organizations:*", "account:*", "billing:*", "aws-portal:*", "payments:*", "sso:*", "sso-directory:*", "identitystore:*",
        "iam:CreateAccountAlias", "iam:DeleteAccountAlias", "iam:UpdateAccountPasswordPolicy", "iam:DeleteAccountPasswordPolicy",
        "iam:CreateLoginProfile", "iam:UpdateLoginProfile",
        // Added 2026-11-15: account-wide settings of the real account, never changed by a recording.
        "iam:SetSecurityTokenServicePreferences", "sts:AssumeRoot", "sns:SetSMSAttributes", "sns:OptInPhoneNumber",
        "logs:PutAccountPolicy", "logs:DeleteAccountPolicy", "kinesis:UpdateAccountSettings", "ssm:UpdateServiceSetting",
        "ssm:ResetServiceSetting",
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
