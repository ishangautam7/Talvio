import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Page, PageHeader } from "@/components/ui/Page";
import { InfoTip } from "@/components/ui/InfoTip";
import { Freeze } from "@/components/shell/Freeze";
import {
  MODEL_SLOTS,
  useNavigation,
  type ModelSlot,
} from "@/components/shell/navigation";
import { useModelSlots } from "@/components/shell/useModelSlots";
import { SlotLogo, useSlotStatusText } from "@/components/shell/SlotVisuals";
import { MODEL_TABS } from "./models/ModelTabs";
import { USED_BY_KEYS } from "./models/usedBy";
import { RetiredSelectionPrompt } from "@/components/settings/models/RetiredSelectionPrompt";
import { FreeCreditNotice } from "@/components/freeCredit/FreeCredit";

/** "Used by dictation, the assistant and meetings." — behind the tab's (i). */
const UsedByTip: React.FC<{ slot: ModelSlot }> = ({ slot }) => {
  const { t } = useTranslation();
  const features = USED_BY_KEYS[slot]
    .map((key) => t(`modelsHub.usedBy.${key}`))
    .join(", ");
  return (
    <p className="flex items-center gap-1 text-[0.8125rem] text-muted">
      <span>{t(`modelsHub.slots.${slot}.description`)}</span>
      <InfoTip text={t("models.usedBy", { features })} />
    </p>
  );
};

/**
 * The four jobs as cards across the top of Models: what each is called and
 * what is doing it right now. They are the page's tabs (WAI-ARIA tabs, arrow
 * keys move between them), so the switcher doubles as an overview.
 */
const JobSwitcher: React.FC<{
  value: ModelSlot;
  onChange: (slot: ModelSlot) => void;
}> = ({ value, onChange }) => {
  const { t } = useTranslation();
  const slots = useModelSlots();
  const statusText = useSlotStatusText();
  const refs = useRef<Partial<Record<ModelSlot, HTMLButtonElement | null>>>({});

  const onKeyDown = (event: React.KeyboardEvent, index: number) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const rtl = document.documentElement.dir === "rtl";
    const forward = (event.key === "ArrowRight") !== rtl;
    const next =
      MODEL_SLOTS[
        (index + (forward ? 1 : MODEL_SLOTS.length - 1)) % MODEL_SLOTS.length
      ];
    onChange(next);
    refs.current[next]?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label={t("nav.models")}
      className="grid grid-cols-2 gap-2.5 @3xl:grid-cols-4"
    >
      {MODEL_SLOTS.map((slot, index) => {
        const summary = slots[slot];
        const selected = slot === value;
        const attention = summary.active && !summary.ready;
        const model =
          summary.active && summary.ready && summary.modelLabel
            ? summary.modelLabel
            : statusText(summary);
        return (
          <button
            key={slot}
            ref={(element) => {
              refs.current[slot] = element;
            }}
            type="button"
            role="tab"
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(slot)}
            onKeyDown={(event) => onKeyDown(event, index)}
            className="job-card flex min-w-0 cursor-pointer flex-col items-start rounded-xl px-3.5 py-3 text-start"
          >
            <span className="w-full min-w-0">
              <span className="flex w-full items-center gap-1.5">
                <span className="job-card-name eyebrow block min-w-0 truncate">
                  {t(`modelsHub.slots.${slot}.name`)}
                </span>
                {attention && (
                  <span
                    className="h-1.5 w-1.5 shrink-0 rounded-full bg-warning"
                    aria-label={t("modelsHub.needsSetup")}
                  />
                )}
              </span>
              <span className="mt-1.5 flex min-w-0 items-center gap-2">
                <SlotLogo summary={summary} size="sm" />
                <span className="job-card-model block truncate text-sm font-medium">
                  {model}
                </span>
              </span>
            </span>
          </button>
        );
      })}
    </div>
  );
};

/**
 * Models: one tab per job — speech to text, AI cleanup, assistant, voice.
 *
 * Tabs keep every job one click from the others, and arriving here from
 * another page (Home's model list, AI cleanup's picker) leaves a real Back
 * link to that page in the header. Adding a model file lives with the lists it
 * adds to, not in the header of every tab (it meant nothing on Voice).
 *
 * Tabs stay mounted once opened (frozen while hidden) so flipping between them
 * does not refetch hardware info or rebuild a model list.
 */
export const ModelsPage: React.FC = () => {
  const { t } = useTranslation();
  const { modelSlot, setModelTab } = useNavigation();
  const tab: ModelSlot = modelSlot ?? "stt";
  const [visited, setVisited] = useState<ModelSlot[]>([tab]);
  const topRef = useRef<HTMLDivElement>(null);
  if (!visited.includes(tab)) setVisited([...visited, tab]);

  // A tab switch starts the new tab at its top, not mid-way down the last one.
  const previousTab = useRef(tab);
  useEffect(() => {
    if (previousTab.current === tab) return;
    previousTab.current = tab;
    const layer = topRef.current?.closest("[data-page]");
    if (layer && layer.scrollTop > (topRef.current?.offsetTop ?? 0)) {
      layer.scrollTo({ top: 0 });
    }
  }, [tab]);

  return (
    <Page>
      <div ref={topRef} />
      <PageHeader
        title={t("nav.models")}
        feature="models"
        description={t("modelsHub.privacyNote")}
      />

      <JobSwitcher value={tab} onChange={setModelTab} />

      <div className="mt-6">
        {visited.map((slot) => {
          const Content = MODEL_TABS[slot];
          const active = slot === tab;
          return (
            <div
              key={slot}
              role="tabpanel"
              hidden={!active}
              className={active ? "tab-reveal" : undefined}
            >
              <div className="mb-6">
                <UsedByTip slot={slot} />
              </div>
              <FreeCreditNotice job={slot} className="mb-6" />
              {slot === "stt" && (
                <RetiredSelectionPrompt placement="models" className="mb-6" />
              )}
              <Freeze freeze={!active}>
                <Content />
              </Freeze>
            </div>
          );
        })}
      </div>
    </Page>
  );
};
