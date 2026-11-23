// What EventBridge answers, as the pages read it: only the members they use (crates/aws-eventbridge answers the rest).

export type Tag = { Key: string; Value: string };

export type RuleItem = { Name: string; Arn?: string; State?: string; Description?: string; EventPattern?: string; ScheduleExpression?: string; EventBusName?: string; ManagedBy?: string };
export type Rules = { Rules?: RuleItem[]; NextToken?: string };

export type TargetItem = {
  Id: string;
  Arn: string;
  Input?: string;
  InputPath?: string;
  InputTransformer?: { InputPathsMap?: Record<string, string>; InputTemplate: string };
  RetryPolicy?: { MaximumRetryAttempts?: number; MaximumEventAgeInSeconds?: number };
  DeadLetterConfig?: { Arn?: string };
  HttpParameters?: { PathParameterValues?: string[]; HeaderParameters?: Record<string, string>; QueryStringParameters?: Record<string, string> };
};
export type Targets = { Targets?: TargetItem[] };

export type BusItem = { Name: string; Arn: string; Description?: string; CreationTime?: number; LastModifiedTime?: number; KmsKeyIdentifier?: string; DeadLetterConfig?: { Arn?: string } };
export type Buses = { EventBuses?: BusItem[] };

export type ArchiveItem = { ArchiveName: string; EventSourceArn: string; State?: string; StateReason?: string; RetentionDays?: number; SizeBytes?: number; EventCount?: number; CreationTime?: number; Description?: string; EventPattern?: string };
export type Archives = { Archives?: ArchiveItem[] };

export type ReplayItem = { ReplayName: string; EventSourceArn: string; State?: string; StateReason?: string; EventStartTime?: number; EventEndTime?: number; EventLastReplayedTime?: number; ReplayStartTime?: number; ReplayEndTime?: number };
export type Replays = { Replays?: ReplayItem[] };

export type ApiDestItem = { Name: string; ApiDestinationArn: string; ApiDestinationState?: string; ConnectionArn: string; InvocationEndpoint: string; HttpMethod: string; InvocationRateLimitPerSecond?: number; CreationTime?: number };
export type ApiDests = { ApiDestinations?: ApiDestItem[] };

export type ConnectionItem = { Name: string; ConnectionArn: string; ConnectionState?: string; AuthorizationType?: string; CreationTime?: number };
export type Connections = { Connections?: ConnectionItem[] };

export type PutEventsAnswer = { FailedEntryCount?: number; Entries?: { EventId?: string; ErrorCode?: string; ErrorMessage?: string }[] };
export type TestAnswer = { Result?: boolean };
export type Aliases = { Aliases?: { AliasName: string; TargetKeyId?: string }[] };
