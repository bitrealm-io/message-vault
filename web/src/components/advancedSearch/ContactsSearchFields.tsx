import { useId } from "react";
import type { Key } from "react-aria-components";
import { parseSelectKey } from "../../lib/selectKey";
import Checkbox from "../Checkbox";
import Select, { ListBoxItem as SelectListBoxItem } from "../Select";
import {
  compactFieldTriggerClass,
  compactSelectItemClassName,
  contactStackClass,
  inputClass,
  labelClass,
} from "./advancedSearchStyles";
import type { ActivityFilter, DateBoundFilter } from "./buildAdvancedQuery";
import { EMPTY_DATE_BOUND } from "./buildAdvancedQuery";
import DateBoundField from "./DateBoundField";
import ServiceMultiSelect from "./ServiceMultiSelect";

export default function ContactsSearchFields({
  contactName,
  onContactNameChange,
  contactNameSaved,
  onContactNameSavedChange,
  handle,
  onHandleChange,
  handleSaved,
  onHandleSavedChange,
  noPreferredName,
  onNoPreferredNameChange,
  noHandle,
  onNoHandleChange,
  services,
  onServicesChange,
  firstMessageBound,
  onFirstMessageBoundChange,
  lastMessageBound,
  onLastMessageBoundChange,
  activity,
  onActivityChange,
  lockedByNoHandle,
  onLockedByNoHandleChange,
}: {
  contactName: string;
  onContactNameChange: (value: string) => void;
  contactNameSaved: string;
  onContactNameSavedChange: (value: string) => void;
  handle: string;
  onHandleChange: (value: string) => void;
  handleSaved: string;
  onHandleSavedChange: (value: string) => void;
  noPreferredName: boolean;
  onNoPreferredNameChange: (value: boolean) => void;
  noHandle: boolean;
  onNoHandleChange: (value: boolean) => void;
  services: Key[];
  onServicesChange: (value: Key[]) => void;
  firstMessageBound: DateBoundFilter;
  onFirstMessageBoundChange: (value: DateBoundFilter) => void;
  lastMessageBound: DateBoundFilter;
  onLastMessageBoundChange: (value: DateBoundFilter) => void;
  activity: ActivityFilter;
  onActivityChange: (value: ActivityFilter) => void;
  lockedByNoHandle: {
    services: Key[];
    firstMessageBound: DateBoundFilter;
    lastMessageBound: DateBoundFilter;
    activity: ActivityFilter;
  } | null;
  onLockedByNoHandleChange: (
    value: {
      services: Key[];
      firstMessageBound: DateBoundFilter;
      lastMessageBound: DateBoundFilter;
      activity: ActivityFilter;
    } | null,
  ) => void;
}) {
  const activityId = useId();

  return (
    <div className={contactStackClass}>
      <div className="min-w-0">
        <label className="block">
          <span className={labelClass}>Name</span>
          <input
            className={`${inputClass} ${noPreferredName ? "cursor-not-allowed opacity-40" : ""}`}
            value={contactName}
            disabled={noPreferredName}
            onChange={(e) => onContactNameChange(e.target.value)}
            placeholder={noPreferredName ? undefined : "Pat Lee"}
          />
        </label>
        <Checkbox
          labelClassName="mt-2"
          checked={noPreferredName}
          onChange={(checked) => {
            if (checked) {
              onContactNameSavedChange(contactName);
              onContactNameChange("");
              onNoPreferredNameChange(true);
            } else {
              onContactNameChange(contactNameSaved);
              onContactNameSavedChange("");
              onNoPreferredNameChange(false);
            }
          }}
        >
          No name
        </Checkbox>
      </div>
      <div className="min-w-0">
        <label className="block">
          <span className={labelClass}>Identity</span>
          <input
            className={`${inputClass} ${noHandle ? "cursor-not-allowed opacity-40" : ""}`}
            value={handle}
            disabled={noHandle}
            onChange={(e) => onHandleChange(e.target.value)}
            placeholder={noHandle ? undefined : "+15555550100"}
          />
        </label>
        <Checkbox
          labelClassName="mt-2"
          checked={noHandle}
          onChange={(checked) => {
            if (checked) {
              onHandleSavedChange(handle);
              onHandleChange("");
              onLockedByNoHandleChange({
                services,
                firstMessageBound,
                lastMessageBound,
                activity,
              });
              onServicesChange([]);
              onFirstMessageBoundChange(EMPTY_DATE_BOUND);
              onLastMessageBoundChange(EMPTY_DATE_BOUND);
              onActivityChange("any");
              onNoHandleChange(true);
            } else {
              onHandleChange(handleSaved);
              onHandleSavedChange("");
              if (lockedByNoHandle) {
                onServicesChange(lockedByNoHandle.services);
                onFirstMessageBoundChange(lockedByNoHandle.firstMessageBound);
                onLastMessageBoundChange(lockedByNoHandle.lastMessageBound);
                onActivityChange(lockedByNoHandle.activity);
                onLockedByNoHandleChange(null);
              }
              onNoHandleChange(false);
            }
          }}
        >
          No identity
        </Checkbox>
      </div>
      <ServiceMultiSelect value={services} onChange={onServicesChange} isDisabled={noHandle} />
      <DateBoundField
        label="First message"
        value={firstMessageBound}
        onChange={onFirstMessageBoundChange}
        isDisabled={noHandle}
      />
      <DateBoundField
        label="Last message"
        value={lastMessageBound}
        onChange={onLastMessageBoundChange}
        isDisabled={noHandle}
      />
      <div className={`min-w-0 ${noHandle ? "opacity-40" : ""}`}>
        <label htmlFor={activityId} className={labelClass}>
          Activity
        </label>
        <Select
          id={activityId}
          selectedKey={activity}
          onSelectionChange={(k) => {
            const next = parseSelectKey(k, ["any", "messages", "no-messages"] as const);
            if (next) onActivityChange(next);
          }}
          aria-label="Activity"
          className="w-full min-w-0"
          size="sm"
          triggerClassName={compactFieldTriggerClass}
          isDisabled={noHandle}
        >
          <SelectListBoxItem id="any" className={compactSelectItemClassName}>
            Any
          </SelectListBoxItem>
          <SelectListBoxItem id="messages" className={compactSelectItemClassName}>
            Has messages
          </SelectListBoxItem>
          <SelectListBoxItem id="no-messages" className={compactSelectItemClassName}>
            Never messaged
          </SelectListBoxItem>
        </Select>
      </div>
    </div>
  );
}
