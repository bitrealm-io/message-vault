import { useId } from "react";
import type { Key } from "react-aria-components";
import { EXPORT_SOURCES } from "../../lib/exportSources";
import { parseSelectKey } from "../../lib/selectKey";
import Select, { ListBoxItem as SelectListBoxItem } from "../Select";
import {
  compactSelectItemClassName,
  inputClass,
  labelClass,
  selectTriggerClass,
} from "./advancedSearchStyles";
import type { CountFilterInput } from "./buildAdvancedQuery";
import ChoiceMultiSelect from "./ChoiceMultiSelect";
import CountField from "./CountField";

export default function MessagesSearchFields({
  nameOrHandle,
  onNameOrHandleChange,
  handle,
  onHandleChange,
  msgType,
  onMsgTypeChange,
  participants,
  onParticipantsChange,
  sources,
  onSourcesChange,
}: {
  nameOrHandle: string;
  onNameOrHandleChange: (value: string) => void;
  handle: string;
  onHandleChange: (value: string) => void;
  msgType: "all" | "direct" | "group";
  onMsgTypeChange: (value: "all" | "direct" | "group") => void;
  participants: CountFilterInput;
  onParticipantsChange: (value: CountFilterInput) => void;
  sources: Key[];
  onSourcesChange: (keys: Key[]) => void;
}) {
  const msgTypeId = useId();

  return (
    <div className="grid grid-cols-2 gap-3">
      <div className="col-span-2">
        <label className="block">
          <span className={labelClass}>Name or title</span>
          <input
            className={inputClass}
            value={nameOrHandle}
            onChange={(e) => onNameOrHandleChange(e.target.value)}
            placeholder="Gregory Coleman"
          />
        </label>
      </div>
      <div>
        <label className="block">
          <span className={labelClass}>Identity</span>
          <input
            className={inputClass}
            value={handle}
            onChange={(e) => onHandleChange(e.target.value)}
            placeholder="+15555550100"
          />
        </label>
      </div>
      <div>
        <label htmlFor={msgTypeId} className={labelClass}>
          Conversation type
        </label>
        <Select
          id={msgTypeId}
          selectedKey={msgType}
          onSelectionChange={(k) => {
            const next = parseSelectKey(k, ["all", "direct", "group"] as const);
            if (next) onMsgTypeChange(next);
          }}
          aria-label="Conversation type"
          size="sm"
          triggerClassName={selectTriggerClass}
        >
          <SelectListBoxItem id="all" className={compactSelectItemClassName}>
            All
          </SelectListBoxItem>
          <SelectListBoxItem id="direct" className={compactSelectItemClassName}>
            Direct
          </SelectListBoxItem>
          <SelectListBoxItem id="group" className={compactSelectItemClassName}>
            Group
          </SelectListBoxItem>
        </Select>
      </div>
      <CountField label="Group participants" value={participants} onChange={onParticipantsChange} />
      {/* `source:` takes the id an import writes on each message, and the
          backups Import reads are named here as Import names them. */}
      <ChoiceMultiSelect
        label="Source"
        items={EXPORT_SOURCES}
        value={sources}
        onChange={onSourcesChange}
      />
    </div>
  );
}
