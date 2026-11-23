// The protocols a subscription can have, as SNS names them and as doze-aws treats each (crates/aws-sns/src/deliver.rs: SQS, Lambda,
// http and https are delivered to; email and email-json wait for a confirmation nobody is sent; sms, application and firehose are
// kept and sent nowhere). Pure: the forms, the tables and the sentences read one table.

export type Protocol = "http" | "https" | "email" | "email-json" | "firehose" | "sqs" | "lambda" | "application" | "sms";

export type ProtocolInfo = {
  id: Protocol;
  /** As the console's list and the forms name it. */
  label: string;
  /** Delivered to here, or not at all. */
  active: boolean;
  /** The words under the option: what it is, and (when it is off) why. */
  about: string;
  /** What the Endpoint is. */
  endpoint: string;
  /** Whether the endpoint owner must confirm. */
  confirms: "never" | "endpoint" | "nobody";
  /** Whether RawMessageDelivery is offered. */
  raw: boolean;
};

const MAIL = "Not available in doze-aws: it would need a mail server. Subscriptions are stored and never delivered.";

/** In the order of the board. */
export const PROTOCOLS: readonly ProtocolInfo[] = [
  { id: "http", label: "HTTP", active: true, about: "Delivery of the JSON-encoded message by HTTP POST. The endpoint is a URL beginning with http://. Delivers locally.", endpoint: "A URL beginning with http://. A local URL works", confirms: "endpoint", raw: true },
  { id: "https", label: "HTTPS", active: true, about: "Delivery of the JSON-encoded message by HTTPS POST. The endpoint is a URL beginning with https://. Delivers locally.", endpoint: "A URL beginning with https://", confirms: "endpoint", raw: true },
  { id: "email", label: "Email", active: false, about: MAIL, endpoint: "An email address", confirms: "nobody", raw: false },
  { id: "email-json", label: "Email-JSON", active: false, about: MAIL, endpoint: "An email address", confirms: "nobody", raw: false },
  { id: "firehose", label: "Amazon Data Firehose", active: false, about: "Not available in doze-aws: it needs Amazon Data Firehose, which doze-aws does not have.", endpoint: "The ARN of a delivery stream", confirms: "never", raw: true },
  { id: "sqs", label: "Amazon SQS", active: true, about: "The endpoint is the ARN of an Amazon SQS queue. Delivers locally; the choice for a FIFO topic.", endpoint: "The ARN of a queue. The choice for a FIFO topic", confirms: "never", raw: true },
  { id: "lambda", label: "AWS Lambda", active: true, about: "The endpoint is the ARN of a Lambda function. Delivers locally.", endpoint: "The ARN of a function", confirms: "never", raw: false },
  { id: "application", label: "Platform application endpoint", active: false, about: "Not available in doze-aws: mobile push needs the platform push services. Messages to one are accepted and sent nowhere.", endpoint: "The ARN of an endpoint", confirms: "never", raw: false },
  { id: "sms", label: "SMS", active: false, about: "Not available in doze-aws: SMS needs a carrier. Messages to a number are accepted and sent nowhere.", endpoint: "A phone number", confirms: "never", raw: false },
];

const BY = new Map<string, ProtocolInfo>(PROTOCOLS.map((p) => [p.id, p]));

export const protocolOf = (id: string): ProtocolInfo | undefined => BY.get(id);

/** `Amazon SQS` for `sqs`; an unknown one as it came. */
export const protocolLabel = (id: string): string => BY.get(id)?.label ?? id;

/** How many protocols cannot be chosen here: `Five of the nine protocols are disabled here`. */
export const disabledCount = (): number => PROTOCOLS.filter((p) => !p.active).length;

const WORDS = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine"];
export const countWord = (n: number): string => WORDS[n] ?? String(n);

/** Whether a subscription of this protocol is delivered to by doze-aws. */
export const delivers = (id: string): boolean => BY.get(id)?.active ?? false;

const STORED_WHY: Record<string, string> = {
  email: "Stored. doze-aws has no mail server, so nothing is sent.",
  "email-json": "Stored. doze-aws has no mail server, so nothing is sent.",
  sms: "Stored. doze-aws has no phone network, so nothing is sent.",
  application: "Stored. doze-aws has no push service, so nothing is sent.",
  firehose: "Stored. doze-aws has no Amazon Data Firehose, so nothing is sent.",
};

/** Why a subscription of this protocol is kept and nothing is delivered to it; null when it is delivered to. */
export const storedWhy = (protocol: string): string | null => STORED_WHY[protocol] ?? null;
