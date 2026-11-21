# Where the console does not do what a board says

The boards of the design canvas (`8fb38ec7-de4e-4570-b8c8-d1eabfe70929`) are the spec for the doze-aws console, with one exception:
a board that says something false of doze-aws, or asks for something doze-aws cannot do, is built to what the backend does. Each
such case is a row here, written by the slice that met it. The canvas is not edited.

A row says which board, what the board says, what the backend does (with the file that shows it), and what was decided.

| Board | What the board says | What the backend does | Decision |
| --- | --- | --- | --- |
| `sqs__queue-detail` | The sentence ends "which holds 1 now" (the dead-letter queue's own message count). | The detail makes the calls of the queue itself; the dead-letter queue's count is another queue's attributes, which no call of this page reads (`crates/aws-console/src/resources/says/sqs.rs`). | The sentence names the dead-letter queue and its maximum receives and leaves the count out. The queue's own page says its count. |
| `sqs__dlq-detail` | "…which sends a message here once its receive count exceeds 5" (the source queue's maximum receives). | The source queue's redrive policy is that queue's attribute, not read from the dead-letter queue's page. | The sentence says "It is the dead-letter queue of pick-lists." and leaves the number out. |
| `sqs__queue-detail`, `sns__topic-detail` | "Wired to" names the rule, publisher or key that feeds the resource (`payment-settled rule, bus larder`). | The strip is built from the resource's own links and lists (`resources/page.rs::wired`): what it points at, not what points at it. | Built to what the descriptor knows. Inbound wiring needs a read in the other services and is not built. |

## How a slice adds a row

Append a row in the table above, in the same commit as the code that departs from the board. Name the board exactly, quote what it
says, say what the backend does and where, and say the decision (built to the backend, or deferred with the owner's reason).
