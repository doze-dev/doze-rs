import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { SubRow } from "./sub-model";
import type { Topic } from "./topic-model";

/** What the pages a topic opens with `?view=` (Edit, Publish message) are given: the topic, its subscriptions, and the ways back. */
export type TopicProps = { d: ResourceDetail; t: Topic; subs: SubRow[]; back: () => void; done: (done: string, tab?: string) => void };
