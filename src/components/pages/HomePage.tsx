import React from "react";
import { useTranslation } from "react-i18next";
import { Page, PageHeader } from "@/components/ui/Page";
import { useModelSlots } from "@/components/shell/useModelSlots";
import { RetiredSelectionPrompt } from "@/components/settings/models/RetiredSelectionPrompt";
import { ShortcutsCard } from "./home/ShortcutsCard";
import { ModelsCard } from "./home/ModelsCard";
import { RecentCard } from "./home/RecentCard";
import { StatusHero } from "./home/StatusHero";
import { partOfDay } from "./home/usageMath";

/**
 * Home: whether Talvio can listen and with what (one card, its numbers along
 * the foot), then the shortcuts, the models doing each job, and the last few
 * things you said. The full usage picture lives on Insights.
 *
 * When dictation cannot run yet the status card says so and carries the one
 * action that fixes it; there is no second banner repeating it above.
 */
export const HomePage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const slots = useModelSlots();
  const now = new Date();
  const greeting = t(`home.greeting.${partOfDay(now.getHours())}`);
  // Today's date, in the app's language, above the greeting.
  const today = new Intl.DateTimeFormat(i18n.language, {
    weekday: "long",
    month: "long",
    day: "numeric",
  }).format(now);
  const needsUpdate = slots.stt.issue === "needs_update";

  return (
    <Page>
      <PageHeader title={greeting} eyebrow={today} />

      {needsUpdate && (
        <RetiredSelectionPrompt placement="home" className="mb-6" />
      )}

      <StatusHero needsModel={!needsUpdate && !slots.stt.ready} />

      <div className="mt-10 grid grid-cols-1 gap-10 @4xl:grid-cols-[minmax(0,1fr)_22rem] @4xl:gap-8">
        <ShortcutsCard />
        <ModelsCard />
      </div>

      <div className="mt-10">
        <RecentCard />
      </div>
    </Page>
  );
};
