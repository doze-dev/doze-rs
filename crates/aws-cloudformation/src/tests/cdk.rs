//! The CDK: its bootstrap stack (the shape `cdk bootstrap` deploys, version 28: conditions, an ECR repository that is skipped, the
//! staging bucket and its policy, the five roles and two policies, the version parameter), and its custom resources.

use serde_json::{Value, json};

use super::World;

const BOOTSTRAP: &str = r#"
Description: This stack includes resources needed to deploy AWS CDK apps into this environment
Parameters:
  TrustedAccounts: {Description: List of AWS accounts that are trusted to publish assets and deploy stacks to this environment, Default: "", Type: CommaDelimitedList}
  CloudFormationExecutionPolicies: {Default: "", Type: CommaDelimitedList}
  FileAssetsBucketName: {Default: "", Type: String}
  FileAssetsBucketKmsKeyId: {Default: "", Type: String}
  ContainerAssetsRepositoryName: {Default: "", Type: String}
  Qualifier: {Default: hnb659fds, Type: String, AllowedPattern: "[A-Za-z0-9_-]{1,10}"}
  PublicAccessBlockConfiguration: {Default: "true", Type: String, AllowedValues: ["true", "false"]}
Conditions:
  HasTrustedAccounts: !Not [!Equals ["", !Join ["", !Ref TrustedAccounts]]]
  HasCloudFormationExecutionPolicies: !Not [!Equals ["", !Join ["", !Ref CloudFormationExecutionPolicies]]]
  HasCustomFileAssetsBucketName: !Not [!Equals ["", !Ref FileAssetsBucketName]]
  CreateNewKey: !Equals ["", !Ref FileAssetsBucketKmsKeyId]
  UseAwsManagedKey: !Equals [AWS_MANAGED_KEY, !Ref FileAssetsBucketKmsKeyId]
  HasCustomContainerAssetsRepositoryName: !Not [!Equals ["", !Ref ContainerAssetsRepositoryName]]
  UsePublicAccessBlockConfiguration: !Equals ["true", !Ref PublicAccessBlockConfiguration]
Resources:
  FileAssetsBucketEncryptionKey:
    Type: AWS::KMS::Key
    Properties:
      KeyPolicy:
        Statement:
          - Action: ["kms:*"]
            Effect: Allow
            Principal: {AWS: !Ref AWS::AccountId}
            Resource: "*"
    Condition: CreateNewKey
  FileAssetsBucketEncryptionKeyAlias:
    Condition: CreateNewKey
    Type: AWS::KMS::Alias
    Properties:
      AliasName: !Sub "alias/cdk-${Qualifier}-assets-key"
      TargetKeyId: !Ref FileAssetsBucketEncryptionKey
  StagingBucket:
    Type: AWS::S3::Bucket
    Properties:
      BucketName: !If [HasCustomFileAssetsBucketName, !Sub "${FileAssetsBucketName}", !Sub "cdk-${Qualifier}-assets-${AWS::AccountId}-${AWS::Region}"]
      AccessControl: Private
      BucketEncryption:
        ServerSideEncryptionConfiguration:
          - ServerSideEncryptionByDefault:
              SSEAlgorithm: aws:kms
              KMSMasterKeyID: !If [CreateNewKey, !Sub "${FileAssetsBucketEncryptionKey.Arn}", !If [UseAwsManagedKey, !Ref AWS::NoValue, !Sub "${FileAssetsBucketKmsKeyId}"]]
      PublicAccessBlockConfiguration: !If [UsePublicAccessBlockConfiguration, {BlockPublicAcls: true, BlockPublicPolicy: true, IgnorePublicAcls: true, RestrictPublicBuckets: true}, !Ref AWS::NoValue]
      VersioningConfiguration: {Status: Enabled}
      LifecycleConfiguration:
        Rules:
          - Id: CleanupOldVersions
            Status: Enabled
            NoncurrentVersionExpiration: {NoncurrentDays: 30}
          - Id: AbortIncompleteMultipartUploads
            Status: Enabled
            AbortIncompleteMultipartUpload: {DaysAfterInitiation: 1}
    UpdateReplacePolicy: Retain
    DeletionPolicy: Retain
  StagingBucketPolicy:
    Type: AWS::S3::BucketPolicy
    Properties:
      Bucket: {Ref: StagingBucket}
      PolicyDocument:
        Id: AccessControl
        Version: "2012-10-17"
        Statement:
          - Sid: AllowSSLRequestsOnly
            Action: s3:*
            Effect: Deny
            Resource: [{Fn::Sub: "${StagingBucket.Arn}"}, {Fn::Sub: "${StagingBucket.Arn}/*"}]
            Condition: {Bool: {aws:SecureTransport: "false"}}
            Principal: "*"
  ContainerAssetsRepository:
    Type: AWS::ECR::Repository
    Properties:
      ImageTagMutability: IMMUTABLE
      RepositoryName: !If [HasCustomContainerAssetsRepositoryName, !Sub "${ContainerAssetsRepositoryName}", !Sub "cdk-${Qualifier}-container-assets-${AWS::AccountId}-${AWS::Region}"]
  FilePublishingRole:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument:
        Statement:
          - Action: sts:AssumeRole
            Effect: Allow
            Principal: {AWS: !Ref AWS::AccountId}
          - !If [HasTrustedAccounts, {Action: sts:AssumeRole, Effect: Allow, Principal: {AWS: !Ref TrustedAccounts}}, !Ref AWS::NoValue]
      RoleName: !Sub "cdk-${Qualifier}-file-publishing-role-${AWS::AccountId}-${AWS::Region}"
      Tags: [{Key: aws-cdk:bootstrap-role, Value: file-publishing}]
  ImagePublishingRole:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument: {Statement: [{Action: sts:AssumeRole, Effect: Allow, Principal: {AWS: !Ref AWS::AccountId}}]}
      RoleName: !Sub "cdk-${Qualifier}-image-publishing-role-${AWS::AccountId}-${AWS::Region}"
  LookupRole:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument: {Statement: [{Action: sts:AssumeRole, Effect: Allow, Principal: {AWS: !Ref AWS::AccountId}}]}
      RoleName: !Sub "cdk-${Qualifier}-lookup-role-${AWS::AccountId}-${AWS::Region}"
      ManagedPolicyArns: [!Sub "arn:${AWS::Partition}:iam::aws:policy/ReadOnlyAccess"]
      Policies:
        - PolicyDocument:
            Statement: [{Sid: DontReadSecrets, Effect: Deny, Action: ["kms:Decrypt"], Resource: "*"}]
            Version: "2012-10-17"
          PolicyName: LookupRolePolicy
  FilePublishingRoleDefaultPolicy:
    Type: AWS::IAM::Policy
    Properties:
      PolicyDocument:
        Statement:
          - Action: ["s3:GetObject*", "s3:PutObject*"]
            Resource: [!GetAtt StagingBucket.Arn, !Sub "${StagingBucket.Arn}/*"]
            Effect: Allow
        Version: "2012-10-17"
      Roles: [{Ref: FilePublishingRole}]
      PolicyName: {Fn::Sub: "cdk-${Qualifier}-file-publishing-role-default-policy-${AWS::AccountId}-${AWS::Region}"}
  DeploymentActionRole:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument: {Statement: [{Action: sts:AssumeRole, Effect: Allow, Principal: {AWS: !Ref AWS::AccountId}}]}
      Policies:
        - PolicyDocument:
            Statement:
              - {Sid: CloudFormationPermissions, Effect: Allow, Action: ["cloudformation:*"], Resource: "*"}
              - {Sid: PassRole, Effect: Allow, Action: iam:PassRole, Resource: {Fn::Sub: "${CloudFormationExecutionRole.Arn}"}}
            Version: "2012-10-17"
          PolicyName: default
      RoleName: !Sub "cdk-${Qualifier}-deploy-role-${AWS::AccountId}-${AWS::Region}"
  CloudFormationExecutionRole:
    Type: AWS::IAM::Role
    Properties:
      AssumeRolePolicyDocument: {Statement: [{Action: sts:AssumeRole, Effect: Allow, Principal: {Service: cloudformation.amazonaws.com}}]}
      ManagedPolicyArns: !If [HasCloudFormationExecutionPolicies, !Ref CloudFormationExecutionPolicies, !If [HasTrustedAccounts, !Ref AWS::NoValue, [!Sub "arn:${AWS::Partition}:iam::aws:policy/AdministratorAccess"]]]
      RoleName: !Sub "cdk-${Qualifier}-cfn-exec-role-${AWS::AccountId}-${AWS::Region}"
  CdkBootstrapVersion:
    Type: AWS::SSM::Parameter
    Properties:
      Type: String
      Name: !Sub "/cdk-bootstrap/${Qualifier}/version"
      Value: "28"
Outputs:
  BucketName: {Value: !Sub "${StagingBucket}"}
  BucketDomainName: {Value: !Sub "${StagingBucket.RegionalDomainName}"}
  FileAssetKeyArn: {Value: !If [CreateNewKey, !Sub "${FileAssetsBucketEncryptionKey.Arn}", !Sub "${FileAssetsBucketKmsKeyId}"]}
  ImageRepositoryName: {Value: !Sub "${ContainerAssetsRepository}"}
  BootstrapVersion: {Value: !GetAtt CdkBootstrapVersion.Value}
"#;

#[tokio::test]
async fn the_bootstrap_stack_deploys_and_skips_only_the_registry() {
    let w = World::new();
    let status = w
        .create("CDKToolkit", Value::String(BOOTSTRAP.into()), &[])
        .await;
    let failed: Vec<_> = w
        .events("CDKToolkit")
        .await
        .into_iter()
        .filter(|e| e.1.ends_with("FAILED"))
        .collect();
    assert_eq!(status, "CREATE_COMPLETE", "{failed:?}");
    let o = w.outputs("CDKToolkit").await;
    assert_eq!(
        o["BucketName"],
        "cdk-hnb659fds-assets-111122223333-us-east-1"
    );
    assert_eq!(o["BootstrapVersion"], "28");
    assert_eq!(
        o["ImageRepositoryName"],
        "cdk-hnb659fds-container-assets-111122223333-us-east-1"
    );
    let p = w
        .call(
            "ssm",
            "GetParameter",
            json!({"Name": "/cdk-bootstrap/hnb659fds/version"}),
        )
        .await
        .unwrap();
    assert_eq!(p["Parameter"]["Value"], "28");
    let role = w
        .call(
            "iam",
            "GetRole",
            json!({"RoleName": "cdk-hnb659fds-deploy-role-111122223333-us-east-1"}),
        )
        .await;
    assert!(role.is_ok());
    let why = w
        .reason("CDKToolkit", "ContainerAssetsRepository", "CREATE_COMPLETE")
        .await;
    assert!(why.starts_with("Not created locally"), "{why}");
}

#[tokio::test]
async fn cdk_custom_resources_are_done_natively() {
    let w = World::new();
    let t = json!({"Resources": {
        "B": {"Type": "AWS::S3::Bucket", "Properties": {"BucketName": "cdk-native"}},
        "Empty": {"Type": "Custom::S3AutoDeleteObjects", "Properties": {"ServiceToken": "arn:aws:lambda:us-east-1:111122223333:function:x", "BucketName": {"Ref": "B"}}},
        "Ssm": {"Type": "Custom::AWS", "Properties": {"ServiceToken": "arn:aws:lambda:us-east-1:111122223333:function:x",
            "Create": json!({"service": "SSM", "action": "putParameter", "parameters": {"Name": "/from-sdk", "Value": "v", "Type": "String"},
                "physicalResourceId": {"id": "sdk-param"}}).to_string(),
            "Delete": json!({"service": "SSM", "action": "deleteParameter", "parameters": {"Name": "/from-sdk"}}).to_string()}},
        "Get": {"Type": "Custom::AWS", "DependsOn": "Ssm", "Properties": {"ServiceToken": "arn:aws:lambda:us-east-1:111122223333:function:x",
            "Create": json!({"service": "@aws-sdk/client-ssm", "action": "GetParameterCommand", "parameters": {"Name": "/from-sdk"},
                "physicalResourceId": {"responsePath": "Parameter.Name"}, "outputPaths": ["Parameter.Value"]}).to_string()}}},
        "Outputs": {"Got": {"Value": {"Fn::GetAtt": ["Get", "Parameter.Value"]}}}});
    assert_eq!(w.create("k1", t, &[]).await, "CREATE_COMPLETE");
    assert_eq!(w.outputs("k1").await["Got"], "v");
    // A bucket with an object in it goes with the stack: the auto-delete resource empties it first.
    let put = json!({"Bucket": "cdk-native", "Key": "x.txt"});
    w.peers
        .call_rest_with_body(
            "test",
            &w.caller,
            ("s3", "PutObject"),
            &put,
            bytes::Bytes::from_static(b"hello"),
        )
        .await
        .unwrap();
    assert_eq!(w.delete("k1").await, "DELETE_COMPLETE");
    assert!(
        w.call("ssm", "GetParameter", json!({"Name": "/from-sdk"}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn resources_of_services_not_running_fail_and_say_so() {
    let w = World::new();
    let t = json!({"Resources": {"M": {"Type": "AWS::StepFunctions::StateMachine", "Properties": {
        "RoleArn": "arn:aws:iam::111122223333:role/x", "Definition": {"StartAt": "A", "States": {"A": {"Type": "Pass", "End": true}}}}}}});
    assert_eq!(w.create("n1", t, &[]).await, "ROLLBACK_COMPLETE");
    let why = w.reason("n1", "M", "CREATE_FAILED").await;
    assert!(why.contains("no service states is running"), "{why}");
    let t = json!({"Resources": {"A": {"Type": "AWS::ApiGatewayV2::Api", "Properties": {"Name": "a", "ProtocolType": "HTTP"}}}});
    assert_eq!(w.create("n2", t, &[]).await, "ROLLBACK_COMPLETE");
}
