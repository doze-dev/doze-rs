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
      ],
      Resource: "*",
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
        `${A}:lambda:${R}:*:function:${P}*`,
        `${A}:lambda:${R}:*:function:${P}*:*`,
        `${A}:lambda:${R}:*:layer:${P}*`,
        `${A}:lambda:${R}:*:layer:${P}*:*`,
        `${A}:lambda:${R}:*:event-source-mapping:*`,
        `${A}:states:${R}:*:stateMachine:${P}*`,
        `${A}:states:${R}:*:execution:${P}*:*`,
        `${A}:states:${R}:*:activity:${P}*`,
        `${A}:kinesis:${R}:*:stream/${P}*`,
        `${A}:kinesis:${R}:*:stream/${P}*/consumer/*`,
        `${A}:cloudwatch:${R}:*:alarm:${P}*`,
        `${A}:cloudwatch::*:dashboard/${P}*`,
        `${A}:cloudformation:${R}:*:stack/${P}*/*`,
        `${A}:cloudformation:${R}:*:stackset/${P}*:*`,
      ],
      Condition: region,
    },
    {
      Sid: "AccountLevelCallsThatCannotBePrefixed",
      Effect: "Allow",
      Action: ["cloudwatch:PutMetricData", "apigateway:*", "execute-api:Invoke"],
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
      Resource: `${A}:kms:${R}:*:alias/${P}*`,
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
