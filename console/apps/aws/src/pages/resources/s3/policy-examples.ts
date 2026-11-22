// Starting points for a bucket policy (the board's "Policy examples"), pure: each is a document for this bucket and this account.
export type PolicyExample = { id: string; title: string; says: string; policy: string };

export function policyExamples(bucket: string, account: string): PolicyExample[] {
  const arn = `arn:aws:s3:::${bucket}`;
  const doc = (statement: object) => JSON.stringify({ Version: "2012-10-17", Statement: [statement] }, null, 2);
  return [
    { id: "role-read", title: "Let a role read objects", says: "Allows one IAM role of this account to get the bucket's objects.", policy: doc({ Sid: "RoleReadsObjects", Effect: "Allow", Principal: { AWS: `arn:aws:iam::${account}:role/lambda-exec` }, Action: "s3:GetObject", Resource: `${arn}/*` }) },
    { id: "account-list", title: "Let an account list the bucket", says: "Allows every identity of this account whose own policies allow it to list the bucket.", policy: doc({ Sid: "AccountListsBucket", Effect: "Allow", Principal: { AWS: `arn:aws:iam::${account}:root` }, Action: "s3:ListBucket", Resource: arn }) },
    { id: "tls-only", title: "Refuse requests that are not encrypted in transit", says: "Denies every request made without TLS (aws:SecureTransport is false).", policy: doc({ Sid: "DenyInsecureTransport", Effect: "Deny", Principal: "*", Action: "s3:*", Resource: [arn, `${arn}/*`], Condition: { Bool: { "aws:SecureTransport": "false" } } }) },
    { id: "public-read", title: "Let anyone read objects", says: "Allows anonymous reads of every object. Block public policy must be off for the bucket, or S3 refuses the policy.", policy: doc({ Sid: "PublicRead", Effect: "Allow", Principal: "*", Action: "s3:GetObject", Resource: `${arn}/*` }) },
  ];
}
