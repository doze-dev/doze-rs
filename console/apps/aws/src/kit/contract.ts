// The JSON shapes the kit and the backend agree on (contract-ui.md). They are the backend's generated types (ts-rs, in
// ../api/types), re-exported under the names the kit uses; the kit adds only what it needs beyond them.
import type { Badge } from "../api/types/Badge";
import type { BlockerGroup } from "../api/types/BlockerGroup";
import type { Blockers } from "../api/types/Blockers";
import type { ParamSpec } from "../api/types/ParamSpec";
import type { Preview } from "../api/types/Preview";
import type { PreviewRequest } from "../api/types/PreviewRequest";
import type { ResourceDetail } from "../api/types/ResourceDetail";
import type { ResourceLink } from "../api/types/ResourceLink";
import type { Tag } from "../api/types/Tag";
import type { TagsChange } from "../api/types/TagsChange";
import type { TagsView } from "../api/types/TagsView";
import type { Tone } from "../api/types/Tone";
import type { VerbSpec } from "../api/types/VerbSpec";
import type { Wired } from "../api/types/Wired";

export type { Blockers, Preview, PreviewRequest, ResourceDetail, ResourceLink, Tag, TagsChange, Wired };
/** `GET` and `PUT .../tags` (contract §3). */
export type TagsAnswer = TagsView;
/** A kind of thing that stands in the way of a delete, or is left behind by it (contract §4). */
export type Blocker = BlockerGroup;
export type BadgeTone = Tone;
export type DetailBadge = Badge;
export type Verb = VerbSpec;
/** A control of a list's filter row (contract §5). */
export type ListParam = ParamSpec;

/** Where a resource's page is: the kit links by address, so it needs no route of the app it is drawn in (the gallery has none). */
export const resourceHref = (l: ResourceLink): string => `/resources/${encodeURIComponent(l.service)}/${encodeURIComponent(l.kind)}/${encodeURIComponent(l.name)}`;

/** The members of a preview request's input, as an object (anything else is none: the server reads it so too). */
export const inputOf = (r: PreviewRequest): Record<string, unknown> => (typeof r.input === "object" && r.input !== null && !Array.isArray(r.input) ? (r.input as Record<string, unknown>) : {});
