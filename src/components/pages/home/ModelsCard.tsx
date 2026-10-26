import React from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight } from "@/components/icons/glyphs";
import { SectionTitle, TextLink } from "@/components/ui/Page";
import { MODEL_SLOTS, useNavigation } from "@/components/shell/navigation";
import {
  useModelSlots,
  type SlotSummary,
} from "@/components/shell/useModelSlots";
import { SlotLogo, useSlotStatusText } from "@/components/shell/SlotVisuals";

/**
 * What is doing each job right now: the job, then the model in words a person
 * would use. The logo says who runs it; the provider and exact id are one
 * hover away. A row opens that job's tab on Models.
 */
const SetupRow: React.FC<{ summary: SlotSummary }> = ({ summary }) => {
  const { t } = useTranslation();
  const { openModelSlot } = useNavigation();
  const statusText = useSlotStatusText();
  const attention = summary.active && !summary.ready;
  const tooltip = [summary.modelId, summary.providerLabel]
    .filter(Boolean)
    .join(" · ");

  return (
    <li className="min-w-0">
      <button
        type="button"
        onClick={() => openModelSlot(summary.slot)}
        title={tooltip || undefined}
        className="group elev-card elev-hover flex h-full w-full cursor-pointer items-center gap-3 rounded-2xl border border-hairline bg-surface px-4 py-3.5 text-start hover:border-hairline-strong focus-visible:outline-none focus-visible:shadow-[var(--focus-ring)]"
      >
        <SlotLogo summary={summary} size="md" />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[0.75rem] font-medium text-muted">
            {t(`modelsHub.slots.${summary.slot}.name`)}
          </span>
          <span
            className={`flex items-center gap-1.5 truncate text-[0.9375rem] font-medium ${
              attention
                ? "text-warning"
                : summary.active
                  ? "text-ink"
                  : "text-muted"
            }`}
          >
            {attention && (
              <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-warning" />
            )}
            <span className="truncate">
              {summary.active && summary.ready && summary.modelLabel
                ? summary.modelLabel
                : statusText(summary)}
            </span>
          </span>
        </span>
        <ChevronRight
          className="h-4 w-4 shrink-0 text-muted-soft transition-[translate,color] group-hover:translate-x-0.5 group-hover:text-muted rtl:rotate-180 rtl:group-hover:-translate-x-0.5"
          aria-hidden="true"
        />
      </button>
    </li>
  );
};

export const ModelsCard: React.FC = () => {
  const { t } = useTranslation();
  const slots = useModelSlots();
  const { navigate } = useNavigation();

  return (
    <section>
      <SectionTitle
        title={t("home.setup.title")}
        action={
          <TextLink onClick={() => navigate("models")}>
            {t("home.setup.manage")}
          </TextLink>
        }
      />
      <ul className="grid grid-cols-1 gap-2.5 @xl:grid-cols-2 @4xl:grid-cols-1">
        {MODEL_SLOTS.map((slot) => (
          <SetupRow key={slot} summary={slots[slot]} />
        ))}
      </ul>
    </section>
  );
};
