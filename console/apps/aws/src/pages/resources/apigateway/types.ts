// What API Gateway answers, as the pages read it: members as the wire writes them (REST's lists are `item`, the v2 service's `items`;
// REST times are seconds since 1970, the v2 service's ISO text).

export type Cors = { allowOrigins?: string[]; allowHeaders?: string[]; allowMethods?: string[]; exposeHeaders?: string[]; maxAge?: number; allowCredentials?: boolean };

export type RestApi = {
  id: string;
  name: string;
  description?: string;
  createdDate?: number;
  rootResourceId?: string;
  apiKeySource?: string;
  endpointConfiguration?: { types?: string[]; ipAddressType?: string };
};
export type RestApis = { item?: RestApi[]; position?: string };

export type MethodResponse = { statusCode: string; responseParameters?: Record<string, boolean>; responseModels?: Record<string, string> };
export type IntegrationResponse = { statusCode: string; selectionPattern?: string; responseParameters?: Record<string, string>; responseTemplates?: Record<string, string>; contentHandling?: string };
export type Integration = {
  type: string;
  httpMethod?: string;
  uri?: string;
  credentials?: string;
  requestParameters?: Record<string, string>;
  requestTemplates?: Record<string, string>;
  passthroughBehavior?: string;
  contentHandling?: string;
  timeoutInMillis?: number;
  cacheKeyParameters?: string[];
  integrationResponses?: Record<string, IntegrationResponse>;
};
export type RestMethod = {
  httpMethod: string;
  authorizationType?: string;
  authorizerId?: string;
  apiKeyRequired?: boolean;
  operationName?: string;
  requestParameters?: Record<string, boolean>;
  methodResponses?: Record<string, MethodResponse>;
  methodIntegration?: Integration;
};
export type RestResource = { id: string; parentId?: string; pathPart?: string; path: string; resourceMethods?: Record<string, RestMethod> };
export type Resources = { item?: RestResource[]; position?: string };

export type MethodSettings = { loggingLevel?: string; dataTraceEnabled?: boolean; metricsEnabled?: boolean; throttlingRateLimit?: number; throttlingBurstLimit?: number; cachingEnabled?: boolean };
export type Stage = {
  stageName: string;
  deploymentId?: string;
  description?: string;
  variables?: Record<string, string>;
  cacheClusterEnabled?: boolean;
  tracingEnabled?: boolean;
  webAclArn?: string;
  clientCertificateId?: string;
  methodSettings?: Record<string, MethodSettings>;
  accessLogSettings?: { format?: string; destinationArn?: string };
  createdDate?: number;
  lastUpdatedDate?: number;
};
export type Stages = { item?: Stage[] };
export type Deployment = { id: string; description?: string; createdDate?: number };
export type Deployments = { item?: Deployment[] };
export type Authorizer = { id: string; name: string; type: string; authorizerUri?: string; authorizerCredentials?: string; identitySource?: string; identityValidationExpression?: string; authorizerResultTtlInSeconds?: number; providerARNs?: string[] };
export type Authorizers = { item?: Authorizer[] };

export type HttpApi = { apiId: string; name: string; description?: string; protocolType: string; apiEndpoint?: string; createdDate?: string; ipAddressType?: string; routeSelectionExpression?: string; corsConfiguration?: Cors };
export type HttpApis = { items?: HttpApi[]; nextToken?: string };
export type Route = { routeId: string; routeKey: string; target?: string; authorizationType?: string; authorizerId?: string; authorizationScopes?: string[]; apiKeyRequired?: boolean };
export type Routes = { items?: Route[] };
export type HttpIntegration = { integrationId: string; integrationType: string; integrationUri?: string; integrationMethod?: string; payloadFormatVersion?: string; timeoutInMillis?: number };
export type HttpIntegrations = { items?: HttpIntegration[] };
export type HttpAuthorizer = { authorizerId: string; name: string; authorizerType: string; authorizerUri?: string; authorizerPayloadFormatVersion?: string; enableSimpleResponses?: boolean; identitySource?: string[]; authorizerResultTtlInSeconds?: number; jwtConfiguration?: { issuer?: string; audience?: string[] } };
export type HttpAuthorizers = { items?: HttpAuthorizer[] };
export type HttpStage = { stageName: string; autoDeploy?: boolean; deploymentId?: string; description?: string; stageVariables?: Record<string, string>; createdDate?: string; lastUpdatedDate?: string; lastDeploymentStatusMessage?: string };
export type HttpStages = { items?: HttpStage[] };
export type HttpDeployment = { deploymentId: string; autoDeployed?: boolean; createdDate?: string; description?: string; deploymentStatus?: string };
export type HttpDeployments = { items?: HttpDeployment[] };

export type UsagePlan = { id: string; name: string; description?: string; apiStages?: { apiId: string; stage: string }[]; throttle?: { rateLimit?: number; burstLimit?: number }; quota?: { limit?: number; offset?: number; period?: string } };
export type UsagePlans = { item?: UsagePlan[]; position?: string };
export type PlanKey = { id: string; type?: string; name?: string; value?: string };
export type PlanKeys = { item?: PlanKey[] };
export type ApiKey = { id: string; name?: string; enabled?: boolean };
export type ApiKeys = { item?: ApiKey[] };

/** Either kind of API, as the list draws it. */
export type AnyApi = { id: string; name: string; description: string; protocol: "REST" | "HTTP" | "WEBSOCKET"; endpointType: string; created: number | null };
