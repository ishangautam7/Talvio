import type { PageId } from "@/components/shell/navigation";
import type { Glyph } from "./base";
import {
  AssistantGlyph,
  CleanupGlyph,
  CommandsGlyph,
  DictionaryGlyph,
  HistoryGlyph,
  HomeGlyph,
  InsightsGlyph,
  MeetingsGlyph,
  ModelsGlyph,
} from "./features";

/** Every page's glyph, for anything that names a page (the navigation, the
 *  palette, a page's title tile). */
export const PAGE_GLYPHS: Record<PageId, Glyph> = {
  home: HomeGlyph,
  insights: InsightsGlyph,
  history: HistoryGlyph,
  assistant: AssistantGlyph,
  commands: CommandsGlyph,
  meetings: MeetingsGlyph,
  cleanup: CleanupGlyph,
  dictionary: DictionaryGlyph,
  models: ModelsGlyph,
};
