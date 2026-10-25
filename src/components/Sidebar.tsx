import React from "react";
import { useTranslation } from "react-i18next";
import { HelpGlyph, SettingsGlyph, type Glyph } from "./icons/glyphs";
import { PAGE_GLYPHS } from "./icons/glyphs/pages";
import DownloadIndicator from "./model-selector/DownloadIndicator";
import { useFeedbackDialog } from "./feedback/feedbackStore";
import { useNavigation, type PageId } from "./shell/navigation";
import { TONE_DOT, useListeningStatus } from "./shell/useListeningStatus";
import { UpdatePill } from "./update-checker/UpdatePill";

interface NavItem {
  id: PageId;
  labelKey: string;
  icon: Glyph;
}

const item = (id: PageId): NavItem => ({
  id,
  labelKey: `nav.${id}`,
  icon: PAGE_GLYPHS[id],
});

/**
 * Primary navigation, in the groups people use it in: where you start and
 * look back (Home, Insights), dictation and what shapes it (History, AI
 * cleanup, Dictionary), the assistant and what it does for you (Assistant,
 * Commands, Meetings), and below a rule the one page that configures models.
 * Everything else lives behind Settings at the foot, and every page is one
 * search away in the command palette (Ctrl+K).
 */
export const NAV_GROUPS: NavItem[][] = [
  [item("home"), item("insights")],
  [item("history"), item("cleanup"), item("dictionary")],
  [item("assistant"), item("commands"), item("meetings")],
];

export const NAV_ITEMS: NavItem[] = NAV_GROUPS.flat();

export const MODELS_ITEM: NavItem = item("models");

/**
 * One page in the navigation: its glyph on a small tile of its own colour,
 * and its name. The page you are on is washed in that colour with the tile
 * filled solid. On a narrow window the names fold away and the tiles stay,
 * with the name as the tooltip.
 */
const NavButton: React.FC<{
  item: NavItem;
  active: boolean;
  onClick: () => void;
}> = ({ item, active, onClick }) => {
  const { t } = useTranslation();
  const Icon = item.icon;
  const label = t(item.labelKey);
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      title={label}
      onClick={onClick}
      data-hue={item.id}
      data-active={active}
      className="nav-item flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-[0.75rem] px-1.5 text-[0.875rem] max-lg:justify-center max-lg:px-0"
    >
      <span className="nav-tile grid h-[1.625rem] w-[1.625rem] shrink-0 place-items-center rounded-[0.5625rem]">
        <Icon className="h-[1.0625rem] w-[1.0625rem]" />
      </span>
      <span className="min-w-0 flex-1 truncate text-start max-lg:hidden">
        {label}
      </span>
    </button>
  );
};

/**
 * What Talvio is listening with, at the foot of the navigation: the speech
 * model's name and state, with a light that is green while it can listen. A
 * click opens its settings. On a narrow window only the light's tile stays.
 */
const ListeningStatus: React.FC = () => {
  const { openModelSlot } = useNavigation();
  const { tone, name, statusText, label } = useListeningStatus();
  return (
    <button
      type="button"
      onClick={() => openModelSlot("stt")}
      title={label}
      aria-label={label}
      className="listen-card flex w-full cursor-pointer items-center gap-2.5 rounded-[0.875rem] p-2 text-start max-lg:justify-center max-lg:p-1.5"
    >
      <span
        data-hue="home"
        className="grid h-8 w-8 shrink-0 place-items-center rounded-[0.625rem] hue-tile"
      >
        <span
          aria-hidden="true"
          className={`live-bars ${tone === "ready" ? "is-live" : ""}`}
        >
          <span />
          <span />
          <span />
          <span />
          <span />
        </span>
      </span>
      <span className="min-w-0 flex-1 max-lg:hidden">
        <span className="block truncate text-[0.8125rem] font-medium text-ink">
          {name}
        </span>
        <span className="flex items-center gap-1.5 text-xs text-muted">
          <span
            aria-hidden="true"
            className={`h-1.5 w-1.5 shrink-0 rounded-full ${TONE_DOT[tone]}`}
          />
          <span className="truncate">{statusText}</span>
        </span>
      </span>
    </button>
  );
};

export const Sidebar: React.FC = () => {
  const { t } = useTranslation();
  const { page, navigateRoot, openSettings, settingsTab } = useNavigation();
  const showFeedback = useFeedbackDialog((s) => s.show);

  return (
    <aside className="relative z-20 flex h-full w-[15rem] shrink-0 flex-col ps-2.5 pe-0 pb-2.5 max-lg:w-[4.5rem]">
      <div className="nav-card flex min-h-0 flex-1 flex-col rounded-[1.25rem] p-2">
        {/* The pages scroll on their own when the window is short; the foot
            does not, so its popovers are never clipped. */}
        <div className="no-scrollbar min-h-0 flex-1 overflow-y-auto">
          <nav className="flex flex-col" aria-label={t("nav.label")}>
            {NAV_GROUPS.map((group, index) => (
              <div
                key={group[0].id}
                className={`flex flex-col gap-0.5 ${index > 0 ? "mt-2.5 border-t border-hairline pt-2.5" : ""}`}
              >
                {group.map((entry) => (
                  <NavButton
                    key={entry.id}
                    item={entry}
                    active={page === entry.id}
                    onClick={() => navigateRoot(entry.id)}
                  />
                ))}
              </div>
            ))}
            <div className="mt-2.5 border-t border-hairline pt-2.5">
              <NavButton
                item={MODELS_ITEM}
                active={page === "models"}
                onClick={() => navigateRoot("models")}
              />
            </div>
          </nav>
        </div>

        <div className="mt-2 flex shrink-0 flex-col gap-1.5">
          <UpdatePill collapsed={false} />
          <div className="flex justify-start empty:hidden max-lg:justify-center">
            <DownloadIndicator placement="start" />
          </div>
          <ListeningStatus />
          <div className="flex items-center gap-1 max-lg:flex-col">
            <button
              type="button"
              onClick={() => openSettings()}
              aria-haspopup="dialog"
              aria-expanded={settingsTab !== null}
              title={t("nav.settings")}
              className="nav-plain group flex h-9 min-w-0 flex-1 cursor-pointer items-center gap-2.5 rounded-[0.75rem] px-1.5 text-[0.875rem] max-lg:w-9 max-lg:flex-none max-lg:justify-center max-lg:px-0"
            >
              <span className="grid h-[1.625rem] w-[1.625rem] shrink-0 place-items-center">
                <SettingsGlyph className="h-[1.125rem] w-[1.125rem] transition-transform duration-500 group-hover:rotate-90 motion-reduce:transition-none" />
              </span>
              <span className="truncate max-lg:hidden">{t("nav.settings")}</span>
            </button>
            <button
              type="button"
              onClick={() => showFeedback()}
              aria-haspopup="dialog"
              aria-label={t("feedback.button")}
              title={t("feedback.button")}
              className="nav-plain grid h-9 w-9 shrink-0 cursor-pointer place-items-center rounded-[0.75rem]"
            >
              <HelpGlyph className="h-[1.125rem] w-[1.125rem]" />
            </button>
          </div>
        </div>
      </div>
    </aside>
  );
};
