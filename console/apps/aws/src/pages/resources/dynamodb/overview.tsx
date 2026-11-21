// A table's Overview tab (dynamodb__table-detail): the fields of DescribeTable, in two cards, and the table's recent calls.
import { dateTimeUtc, number } from "@doze/ui/lib/format";
import { RuleHelp } from "../../../kit";
import { Calls } from "../detail-page";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Card } from "../deep/parts";
import { encryptionWords, keyWords, modeWords, type TableInfo } from "./model";
import { Facts, OnOff, WithSub } from "./tab-parts";
import { StatusDot } from "@doze/ui/components/app/status-dot";

export function Overview({ t, d }: { t: TableInfo; d: ResourceDetail }) {
  return (
    <div className="grid gap-6">
      <Card title="Table details" lede="Fields of the DescribeTable response for this table." testId="table-details">
        <div className="grid gap-x-12 md:grid-cols-2">
          <Facts rows={[["Partition key", <span className="font-mono text-[13.5px]" key="p">{keyWords(t.hash)}</span>], ["Sort key", t.range ? <span className="font-mono text-[13.5px]" key="s">{keyWords(t.range)}</span> : null], ["Capacity mode", modeWords(t)]]} />
          <Facts rows={[["TableStatus", <StatusDot key="s" tone={t.status === "ACTIVE" ? "ok" : "warn"}>{t.status.charAt(0) + t.status.slice(1).toLowerCase()}</StatusDot>], ["ItemCount", number(t.items)], ["Deletion protection", t.protect ? "On" : "Off"]]} />
        </div>
        <RuleHelp className="mt-4 max-w-[40rem]">
          On AWS the item count is <b>not live</b>: DynamoDB updates it about every six hours, so recent writes may not be counted yet. doze-aws counts live.
        </RuleHelp>
      </Card>
      <Card title="Additional details" testId="table-additional">
        <Facts
          rows={[
            ["TableArn", <span className="font-mono text-[13.5px]" key="a">{t.arn}</span>],
            ["Encryption type", <WithSub key="e" sub="Stored, not applied locally.">{encryptionWords(t.encryption)}</WithSub>],
            ["StreamEnabled", <OnOff key="s" on={t.stream.enabled} />],
            ["CreationDateTime", t.createdMs ? dateTimeUtc(t.createdMs) : null],
          ]}
        />
      </Card>
      <Calls d={d} />
    </div>
  );
}
