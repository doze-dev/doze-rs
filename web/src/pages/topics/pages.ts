// The Topics pages as one lazily loaded chunk (they bring TanStack Table with them): the router imports this module only
// when a Topics address is opened.
export { TopicsPage } from "./list";
export { TopicLayout } from "./frame";
export { MessagesPage } from "./messages";
export { PartitionsPage } from "./partitions";
export { ConsumersPage } from "./consumers";
export { LogPage } from "./log";
export { ProducersPage } from "./producers";
export { TxnPage, TxnHeldPage } from "./txn";
export { TopicConfigPage } from "./config";
