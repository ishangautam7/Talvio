import React from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Gift } from "@/components/icons/glyphs";
import type { FreeCreditJob } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Callout } from "@/components/ui/Callout";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { useSettings } from "@/hooks/useSettings";
import { useNavigation, type ModelSlot } from "@/components/shell/navigation";
import { useFreeCredit } from "@/stores/freeCreditStore";
import {
  FREE_CREDIT_JOBS,
  formatUsd,
  formatUsdAmount,
  freeCreditView,
  offersFreeCredit,
  remainingFraction,
  type FreeCreditView,
} from "@/lib/freeCredit";

/** Where a job's own provider is chosen, for "Add your own key". */
const SLOT_FOR_JOB: Record<FreeCreditJob, ModelSlot | null> = {
  stt: "stt",
  cleanup: "cleanup",
  assistant: "assistant",
  command: null,
  voice: "voice",
};

/** "Free credit: $0.73 of $1.00 left", with a meter under it. */
const BalanceLine: React.FC<{ view: FreeCreditView }> = ({ view }) => {
  const { t, i18n } = useTranslation();
  if (view.kind === "loading") {
    return <p className="text-sm text-muted">{t("freeCredit.checking")}</p>;
  }
  if (view.kind !== "available") return null;
  const fraction = remainingFraction(view);
  const text = t("freeCredit.balance", {
    remaining: formatUsd(view.remaining, i18n.language),
    limit: formatUsd(view.limit, i18n.language),
  });
  return (
    <div className="min-w-0 space-y-1.5">
      <p className="text-sm text-ink">{text}</p>
      <div
        role="meter"
        aria-label={text}
        aria-valuemin={0}
        aria-valuemax={view.limit}
        aria-valuenow={view.remaining}
        className="h-1 w-full max-w-[14rem] overflow-hidden rounded-full bg-surface-strong"
      >
        <div
          className="h-full rounded-full bg-[var(--color-accent-fill)]"
          style={{ width: `${Math.round(fraction * 100)}%` }}
        />
      </div>
    </div>
  );
};

/** Points one job at Talvio free and refreshes the settings it changed. */
const useChooseFree = () => {
  const { t } = useTranslation();
  const { refreshSettings } = useSettings();
  const { choose, choosing } = useFreeCredit();
  const run = async (job: FreeCreditJob) => {
    const error = await choose(job);
    if (error) {
      toast.error(t("freeCredit.failed"));
      return false;
    }
    await refreshSettings();
    return true;
  };
  return { run, choosing };
};

/** Used up or switched off: say so, and lead to adding a key of one's own. */
const OutNotice: React.FC<{
  view: FreeCreditView;
  job?: FreeCreditJob;
  className?: string;
}> = ({ view, job, className }) => {
  const { t } = useTranslation();
  const { openModelSlot, navigate } = useNavigation();
  if (view.kind !== "exhausted" && view.kind !== "disabled") return null;
  const slot = job ? SLOT_FOR_JOB[job] : "assistant";
  return (
    <Callout
      tone="warning"
      className={className}
      action={
        <Button
          size="sm"
          variant="secondary"
          onClick={() => (slot ? openModelSlot(slot) : navigate("commands"))}
        >
          {t("freeCredit.addKey")}
        </Button>
      }
    >
      {t(
        view.kind === "exhausted"
          ? "freeCredit.exhausted"
          : "freeCredit.disabled",
      )}
    </Callout>
  );
};

/**
 * Settings → Account: what is left of the free credit, and every job it can
 * do, each either "In use" or one click from using it.
 */
export const FreeCreditGroup: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { status } = useFreeCredit();
  const { run, choosing } = useChooseFree();
  const view = freeCreditView(status);
  if (view.kind === "signed_out") return null;
  const amount = formatUsdAmount(
    view.kind === "available" || view.kind === "exhausted" ? view.limit : 1,
    i18n.language,
  );

  return (
    <SettingsGroup
      title={t("freeCredit.title")}
      description={t("freeCredit.explain", { amount })}
    >
      <div className="flex items-center gap-3 px-5 py-3.5">
        <Gift className="h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          {view.kind === "available" || view.kind === "loading" ? (
            <BalanceLine view={view} />
          ) : (
            <OutNotice view={view} />
          )}
        </div>
      </div>
      {FREE_CREDIT_JOBS.map((job) => {
        const inUse = !!status?.serving[job];
        const offered = offersFreeCredit(status, job);
        if (!inUse && !offered) return null;
        return (
          <div key={job} className="flex items-center gap-3 px-5 py-3">
            <p className="min-w-0 flex-1 text-sm text-body">
              {t(`freeCredit.jobs.${job}`)}
            </p>
            {inUse ? (
              <span className="text-[0.8125rem] text-muted">
                {t("freeCredit.inUse")}
              </span>
            ) : (
              <Button
                size="sm"
                variant="secondary"
                disabled={choosing !== null}
                onClick={() => void run(job)}
              >
                {t("freeCredit.use")}
              </Button>
            )}
          </div>
        );
      })}
    </SettingsGroup>
  );
};

/**
 * Models, on one job's tab: the balance, "Use Talvio free ($1 included)"
 * while that job runs on something else, and what to do once it is spent.
 * Shows nothing to someone who is signed out.
 */
export const FreeCreditNotice: React.FC<{
  job: FreeCreditJob;
  className?: string;
}> = ({ job, className = "" }) => {
  const { t, i18n } = useTranslation();
  const { status } = useFreeCredit();
  const { run, choosing } = useChooseFree();
  const view = freeCreditView(status);
  if (view.kind === "signed_out" || view.kind === "loading") return null;
  if (view.kind === "exhausted" || view.kind === "disabled") {
    // Only where it matters: a job that was running on it.
    return status?.serving[job] ? (
      <OutNotice view={view} job={job} className={className} />
    ) : null;
  }
  const offered = offersFreeCredit(status, job);
  return (
    <div
      className={`flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-hairline bg-surface px-4 py-3 elev-card ${className}`}
    >
      <Gift className="h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
      <div className="min-w-[12rem] flex-1">
        <BalanceLine view={view} />
      </div>
      {offered ? (
        <Button
          size="sm"
          variant="secondary"
          disabled={choosing !== null}
          onClick={() => void run(job)}
        >
          {t("freeCredit.option", {
            amount: formatUsdAmount(view.limit, i18n.language),
          })}
        </Button>
      ) : (
        <span className="text-[0.8125rem] text-muted">
          {t("freeCredit.inUse")}
        </span>
      )}
    </div>
  );
};

/**
 * First-run setup: transcribe on the free credit instead of downloading a
 * model. Offered only to someone signed in with credit left.
 */
export const FreeCreditSetupOffer: React.FC<{
  onChosen: () => void;
  className?: string;
}> = ({ onChosen, className = "" }) => {
  const { t, i18n } = useTranslation();
  const { status } = useFreeCredit();
  const { run, choosing } = useChooseFree();
  const view = freeCreditView(status);
  if (!offersFreeCredit(status, "stt") || view.kind !== "available") {
    return null;
  }
  return (
    <div
      className={`flex flex-wrap items-center gap-x-4 gap-y-2 rounded-2xl border border-hairline bg-surface px-5 py-4 elev-card ${className}`}
    >
      <Gift className="h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
      <p className="min-w-[12rem] flex-1 text-sm text-body">
        {t("freeCredit.setupHint")}
      </p>
      <Button
        size="sm"
        variant="secondary"
        disabled={choosing !== null}
        onClick={() =>
          void run("stt").then((chosen) => {
            if (chosen) onChosen();
          })
        }
      >
        {t("freeCredit.option", {
          amount: formatUsdAmount(view.limit, i18n.language),
        })}
      </Button>
    </div>
  );
};
