import { useParams } from "@tanstack/react-router";
import { TopicConfigTab } from "./config";

/** The route component of `/topics/$topic/config` (its own chunk). */
export function ConfigRoute() {
  const { topic } = useParams({ from: "/topics/$topic/config" });
  return <TopicConfigTab topic={topic} />;
}
