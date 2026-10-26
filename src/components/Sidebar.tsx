import React from "react";
import { useTranslation } from "react-i18next";
import {
  HelpGlyph,
  SettingsGlyph,
  SpeechToTextGlyph,
  type Glyph,
} from "./icons/glyphs";
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
 * Commands, Meetings), and after them the one page that configures models.
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

/** One row's shape, shared by the pages, the listening status and Settings,
 *  so their icons and labels sit on one column. */
const ROW =
  "flex h-8 w-full cursor-pointer items-center gap-2.5 rounded-lg px-2.5 text-[0.875rem] max-lg:justify-center max-lg:px-0";

/**
 * One page in the navigation: a 16px icon in muted ink and its name. The
 * page you are on is a quiet neutral fill with both in full ink. On a narrow
 * window the names fold away and the icons stay, with the name as the
 * tooltip.
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
      data-active={active}
      className={`nav-item ${ROW}`}
    >
      <Icon className="nav-tile h-4 w-4 shrink-0" />
      <span className="min-w-0 flex-1 truncate text-start max-lg:hidden">
        {label}
      </span>
    </button>
  );
};

/**
 * What Talvio is listening with, at the foot of the navigation: the speech
 * model's name, and under it its state behind a small light that is green
 * while it can listen. A click opens its settings. On a narrow window only
 * the icon stays.
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
      className="listen-card flex w-full cursor-pointer items-start gap-2.5 rounded-lg px-2.5 py-2 text-start max-lg:justify-center max-lg:px-0"
    >
      <span className="relative mt-0.5 shrink-0 text-muted">
        <SpeechToTextGlyph className="h-4 w-4" />
        <span
          aria-hidden="true"
          className={`absolute -end-0.5 -bottom-0.5 hidden h-1.5 w-1.5 rounded-full ring-2 ring-canvas max-lg:block ${TONE_DOT[tone]}`}
        />
      </span>
      <span className="min-w-0 flex-1 max-lg:hidden">
        <span className="block truncate text-[0.8125rem] leading-5 font-medium text-ink">
          {name}
        </span>
        <span className="flex items-center gap-1.5 text-xs leading-4 text-muted">
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
    <aside className="relative z-20 flex h-full w-[14rem] shrink-0 flex-col px-3 pb-3 max-lg:w-[3.75rem] max-lg:px-2">
      <div className="nav-card flex min-h-0 flex-1 flex-col">
        {/* The pages scroll on their own when the window is short; the foot
            does not, so its popovers are never clipped. */}
        <div className="no-scrollbar min-h-0 flex-1 overflow-y-auto pt-1">
          <nav className="flex flex-col gap-4" aria-label={t("nav.label")}>
            {[...NAV_GROUPS, [MODELS_ITEM]].map((group) => (
              <div key={group[0].id} className="flex flex-col gap-px">
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
          </nav>
        </div>

        <div className="mt-2 flex shrink-0 flex-col gap-1">
          <UpdatePill collapsed={false} />
          <div className="flex justify-start empty:hidden max-lg:justify-center">
            <DownloadIndicator placement="start" />
          </div>
          <ListeningStatus />
          <div className="mt-1 flex items-center gap-1 border-t border-hairline pt-2 max-lg:flex-col">
            <button
              type="button"
              onClick={() => openSettings()}
              aria-haspopup="dialog"
              aria-expanded={settingsTab !== null}
              title={t("nav.settings")}
              className={`nav-plain min-w-0 flex-1 ${ROW}`}
            >
              <SettingsGlyph className="h-4 w-4 shrink-0" />
              <span className="truncate max-lg:hidden">
                {t("nav.settings")}
              </span>
            </button>
            <button
              type="button"
              onClick={() => showFeedback()}
              aria-haspopup="dialog"
              aria-label={t("feedback.button")}
              title={t("feedback.button")}
              className="nav-plain grid h-8 w-8 shrink-0 cursor-pointer place-items-center rounded-lg"
            >
              <HelpGlyph className="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>
    </aside>
  );
};
