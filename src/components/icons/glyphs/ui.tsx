import { glyph, SOFT, SOLID } from "./base";
import {
  AccountGlyph,
  AdvancedGlyph,
  AudioGlyph,
  CallGlyph,
  DebugGlyph,
  DictionaryGlyph,
  HelpGlyph,
  ModelsGlyph,
  PrivacyGlyph,
  SettingsGlyph,
  VoiceGlyph,
} from "./features";

/**
 * The interface glyphs: every icon the main window used from lucide-react,
 * under the same names, drawn in the Talvio Glyphs style. Objects get one
 * soft body; marks (checks, chevrons, arrows, spinners) are stroke only.
 */

/** A body and its outline, from one path. */
const shape = (d: string) => (
  <>
    <path {...SOFT} d={d} />
    <path d={d} />
  </>
);

const RING = (
  <>
    <circle {...SOFT} cx="12" cy="12" r="8.75" />
    <circle cx="12" cy="12" r="8.75" />
  </>
);

const DOC =
  "M14 3.25H7.75a2.5 2.5 0 0 0-2.5 2.5v12.5a2.5 2.5 0 0 0 2.5 2.5h8.5a2.5 2.5 0 0 0 2.5-2.5V8z";
const DOC_FOLD = "M14 3.25v3.25A1.5 1.5 0 0 0 15.5 8h3.25";

const EYE =
  "M2.75 12C4.8 7.9 8.1 5.75 12 5.75s7.2 2.15 9.25 6.25c-2.05 4.1-5.35 6.25-9.25 6.25S4.8 16.1 2.75 12z";

const BUBBLE =
  "M12 3.5a8.5 8.5 0 0 1 0 17c-1.4 0-2.7-.34-3.86-.94L3.5 20.5l.94-4.64A8.5 8.5 0 0 1 12 3.5z";

const BELL =
  "M6.25 10.25a5.75 5.75 0 0 1 11.5 0c0 3.9 1.1 5.4 2 6.25H4.25c.9-.85 2-2.35 2-6.25z";

const TRASH_BODY =
  "M5.75 6.75l.85 11.6a2.2 2.2 0 0 0 2.2 2.05h6.4a2.2 2.2 0 0 0 2.2-2.05l.85-11.6z";
const TRASH_LID =
  "M4 6.75h16M9.5 6.75V5.1a1.6 1.6 0 0 1 1.6-1.6h1.8a1.6 1.6 0 0 1 1.6 1.6v1.65";

const CALENDAR = (
  <>
    <rect {...SOFT} x="3.5" y="5" width="17" height="15.5" rx="3.25" />
    <rect x="3.5" y="5" width="17" height="15.5" rx="3.25" />
    <path d="M3.5 10h17M8 3v3.75M16 3v3.75" />
  </>
);

const MIC = (
  <>
    <rect {...SOFT} x="8.75" y="2.75" width="6.5" height="11.5" rx="3.25" />
    <rect x="8.75" y="2.75" width="6.5" height="11.5" rx="3.25" />
    <path d="M5.25 11.25a6.75 6.75 0 0 0 13.5 0M12 18v3" />
  </>
);

const PLAY =
  "M7.5 5.65v12.7a1.1 1.1 0 0 0 1.66.94l10.3-6.35a1.1 1.1 0 0 0 0-1.88L9.16 4.71A1.1 1.1 0 0 0 7.5 5.65z";

/** A four-point spark of radius `r`, the assistant glyph's own curve. */
const SPARK = (cx: number, cy: number, r: number) => {
  const f = (n: number) => +(n * r).toFixed(2);
  const [a, b, c, d] = [f(0.084), f(0.54), f(0.46), f(0.916)];
  return (
    `M${cx} ${cy - r}` +
    `c${a} ${b} ${c} ${d} ${r} ${r}` +
    `c${-b} ${a} ${-d} ${c} ${-r} ${r}` +
    `c${-a} ${-b} ${-c} ${-d} ${-r} ${-r}` +
    `c${b} ${-a} ${d} ${-c} ${r} ${-r}z`
  );
};

/* ---------------------------------------------------------------- marks */

export const Check = glyph("check", <path d="M5 12.75l4.5 4.5L19 7.25" />);

export const X = glyph("x", <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />);

export const Plus = glyph("plus", <path d="M12 5.25v13.5M5.25 12h13.5" />);

export const Minus = glyph("minus", <path d="M5.25 12h13.5" />);

export const ChevronDown = glyph(
  "chevron-down",
  <path d="M6.5 9.5 12 15l5.5-5.5" />,
);
export const ChevronUp = glyph(
  "chevron-up",
  <path d="M6.5 14.5 12 9l5.5 5.5" />,
);
export const ChevronRight = glyph(
  "chevron-right",
  <path d="M9.5 6.5 15 12l-5.5 5.5" />,
);
export const ChevronLeft = glyph(
  "chevron-left",
  <path d="M14.5 6.5 9 12l5.5 5.5" />,
);
export const ChevronsUpDown = glyph(
  "chevrons-up-down",
  <path d="M8 9.25l4-4 4 4M8 14.75l4 4 4-4" />,
);

export const ArrowRight = glyph(
  "arrow-right",
  <path d="M4.75 12h14.5M13.5 6.25 19.25 12l-5.75 5.75" />,
);
export const ArrowLeft = glyph(
  "arrow-left",
  <path d="M19.25 12H4.75M10.5 6.25 4.75 12l5.75 5.75" />,
);
export const ArrowUp = glyph(
  "arrow-up",
  <path d="M12 19.25V4.75M6.25 10.5 12 4.75l5.75 5.75" />,
);
export const ArrowUpRight = glyph(
  "arrow-up-right",
  <path d="M6.75 17.25 17.25 6.75M8.5 6.75h8.75v8.75" />,
);

export const CornerDownLeft = glyph(
  "corner-down-left",
  <path d="M19.25 5v6.25a3.25 3.25 0 0 1-3.25 3.25H5M9 10.5l-4 4 4 4" />,
);

export const ExternalLink = glyph(
  "external-link",
  <path d="M13.75 4.5h5.75v5.75M19.5 4.5l-8 8M17.5 14.25v3.5a2.25 2.25 0 0 1-2.25 2.25h-9A2.25 2.25 0 0 1 4 17.75v-9A2.25 2.25 0 0 1 6.25 6.5H9.75" />,
);

export const MoreHorizontal = glyph(
  "more-horizontal",
  <>
    <circle {...SOLID} cx="5.75" cy="12" r="1.6" />
    <circle {...SOLID} cx="12" cy="12" r="1.6" />
    <circle {...SOLID} cx="18.25" cy="12" r="1.6" />
  </>,
);
export const Ellipsis = MoreHorizontal;

export const Loader2 = glyph(
  "loader",
  <path d="M12 3.75A8.25 8.25 0 1 1 3.75 12" />,
);
export const LoaderCircle = Loader2;

export const Loader = glyph(
  "loader-rays",
  <>
    <path d="M12 3.5v3M12 17.5v3M3.5 12h3M17.5 12h3" />
    <path
      strokeOpacity="0.5"
      d="M6 6l2.1 2.1M15.9 15.9 18 18M6 18l2.1-2.1M15.9 8.1 18 6"
    />
  </>,
);

export const RefreshCw = glyph(
  "refresh-cw",
  <path d="M19.5 11a7.5 7.5 0 0 0-13.15-4.4L4.5 8.5M4.5 4.25V8.5h4.25M4.5 13a7.5 7.5 0 0 0 13.15 4.4l1.85-1.9M19.5 19.75V15.5h-4.25" />,
);

export const RotateCw = glyph(
  "rotate-cw",
  <path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3l2.2 2.3M19.5 4.5V9H15" />,
);

export const RotateCcw = glyph(
  "rotate-ccw",
  <path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3L4.5 9M4.5 4.5V9H9" />,
);

export const Download = glyph(
  "download",
  <path d="M12 4v11M7.25 10.5 12 15.25l4.75-4.75M5 19.75h14" />,
);

export const Upload = glyph(
  "upload",
  <path d="M12 15.5V4.5M7.25 9 12 4.25 16.75 9M5 19.75h14" />,
);

export const Square = glyph(
  "square",
  <rect x="5.5" y="5.5" width="13" height="13" rx="3" />,
);

export const Type = glyph(
  "type",
  <path d="M5.25 7.5V5.25h13.5V7.5M12 5.25v13.5M9.25 18.75h5.5" />,
);

export const List = glyph(
  "list",
  <>
    <path d="M9.25 6.5h10.5M9.25 12h10.5M9.25 17.5h10.5" />
    <circle {...SOLID} cx="5" cy="6.5" r="1.2" />
    <circle {...SOLID} cx="5" cy="12" r="1.2" />
    <circle {...SOLID} cx="5" cy="17.5" r="1.2" />
  </>,
);

export const AudioLines = glyph(
  "audio-lines",
  <path d="M4 10.5v3M7.2 7.5v9M10.4 4.5v15M13.6 8.5v7M16.8 6v12M20 10.5v3" />,
);

export const AudioWaveform = glyph(
  "audio-waveform",
  <path d="M3 12h1.75l2.25-5 3 10.5 2.75-13 3 12.5 2-5H21" />,
);

export const Waves = glyph(
  "waves",
  <path d="M3.5 7.5c1.4-1.3 2.85-1.3 4.25 0s2.85 1.3 4.25 0 2.85-1.3 4.25 0 2.85 1.3 4.25 0M3.5 12c1.4-1.3 2.85-1.3 4.25 0s2.85 1.3 4.25 0 2.85-1.3 4.25 0 2.85 1.3 4.25 0M3.5 16.5c1.4-1.3 2.85-1.3 4.25 0s2.85 1.3 4.25 0 2.85-1.3 4.25 0 2.85 1.3 4.25 0" />,
);

export const Languages = glyph(
  "languages",
  <>
    <path d="M3.5 6.5h8M7.5 4.25V6.5M9.75 6.5c-.6 3-2.6 5.2-5.75 6.5M5.5 9.25c1 1.6 2.5 2.8 4.5 3.6" />
    <path d="M12.75 20 16.5 11l3.75 9M14 17h5" />
  </>,
);

export const GitBranch = glyph(
  "git-branch",
  <>
    <circle cx="6.75" cy="5.75" r="2.25" />
    <circle cx="6.75" cy="18.25" r="2.25" />
    <circle cx="17.25" cy="7.25" r="2.25" />
    <path d="M6.75 8v8M17.25 9.5c0 4.6-4.6 5.1-8.6 7.1" />
  </>,
);

export const Link = glyph(
  "link",
  <path d="M10 13.75a4 4 0 0 0 6 .4l2.6-2.6a4 4 0 0 0-5.65-5.65l-1.45 1.4M14 10.25a4 4 0 0 0-6-.4l-2.6 2.6a4 4 0 0 0 5.65 5.65l1.4-1.4" />,
);

export const LogOut = glyph(
  "log-out",
  <path d="M9.5 20H6.75a2.5 2.5 0 0 1-2.5-2.5v-11A2.5 2.5 0 0 1 6.75 4H9.5M15.5 16.5 20 12l-4.5-4.5M19.75 12H9.75" />,
);

export const TextSelect = glyph(
  "text-select",
  <>
    <path d="M4 8V6.25A2.25 2.25 0 0 1 6.25 4H8M16 4h1.75A2.25 2.25 0 0 1 20 6.25V8M20 16v1.75A2.25 2.25 0 0 1 17.75 20H16M8 20H6.25A2.25 2.25 0 0 1 4 17.75V16" />
    <path d="M8.5 10h7M8.5 14h4.5" />
  </>,
);

export const TextCursorInput = glyph(
  "text-cursor-input",
  <path d="M13.5 6.75h4.25A2.75 2.75 0 0 1 20.5 9.5v5a2.75 2.75 0 0 1-2.75 2.75H13.5M6.5 17.25h-.25A2.75 2.75 0 0 1 3.5 14.5v-5a2.75 2.75 0 0 1 2.75-2.75h.25M10 4v16M8 4h4M8 20h4" />,
);

/* -------------------------------------------------------------- circles */

export const Info = glyph(
  "info",
  <>
    {RING}
    <path d="M12 11v5.25" />
    <circle {...SOLID} cx="12" cy="7.9" r="1.05" />
  </>,
);

export const CircleQuestionMark = glyph(
  "circle-question",
  <>
    {RING}
    <path d="M9.6 9.6a2.45 2.45 0 1 1 3.35 2.3c-.6.25-.95.75-.95 1.35v.35" />
    <circle {...SOLID} cx="12" cy="16.6" r="1.05" />
  </>,
);
export const HelpCircle = CircleQuestionMark;

export const CircleCheck = glyph(
  "circle-check",
  <>
    {RING}
    <path d="m8.25 12.25 2.5 2.5 5-5" />
  </>,
);
export const CheckCircle = CircleCheck;

export const CircleAlert = glyph(
  "circle-alert",
  <>
    {RING}
    <path d="M12 7.5v5.25" />
    <circle {...SOLID} cx="12" cy="16.25" r="1.05" />
  </>,
);
export const AlertCircle = CircleAlert;

export const CircleArrowUp = glyph(
  "circle-arrow-up",
  <>
    {RING}
    <path d="M12 16.5v-8.75M8.25 11.25 12 7.5l3.75 3.75" />
  </>,
);

export const CircleUserRound = AccountGlyph;

export const Clock = glyph(
  "clock",
  <>
    {RING}
    <path d="M12 7.5V12l3 1.75" />
  </>,
);

export const History = glyph(
  "history",
  <>
    <path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3L4.5 9M4.5 4.5V9H9" />
    <path d="M12 8v4.25l2.75 1.6" />
  </>,
);

export const Globe = glyph(
  "globe",
  <>
    {RING}
    <path d="M12 3.25c-2.35 2.4-3.5 5.3-3.5 8.75s1.15 6.35 3.5 8.75c2.35-2.4 3.5-5.3 3.5-8.75S14.35 5.65 12 3.25zM3.5 12h17" />
  </>,
);

/** Earth and Globe were two drawings in lucide; one reads better small. */
export const Earth = Globe;

export const Power = glyph(
  "power",
  <path d="M12 3.5v7.75M7.2 6.3a7.5 7.5 0 1 0 9.6 0" />,
);

export const Gauge = glyph(
  "gauge",
  <>
    <path {...SOFT} d="M4.25 17.25a8.25 8.25 0 1 1 15.5 0z" />
    <path d="M4.25 17.25a8.25 8.25 0 1 1 15.5 0" />
    <path d="M12 14.5l3.25-4.25" />
    <circle {...SOLID} cx="12" cy="14.5" r="1.6" />
  </>,
);

export const Radio = glyph(
  "radio",
  <>
    <circle {...SOLID} cx="12" cy="12" r="2" />
    <path d="M8.5 8.5a5 5 0 0 0 0 7M15.5 8.5a5 5 0 0 1 0 7M5.6 5.6a9 9 0 0 0 0 12.8M18.4 5.6a9 9 0 0 1 0 12.8" />
  </>,
);

/* -------------------------------------------------------------- objects */

export const Mic = glyph("mic", MIC);

export const MicOff = glyph(
  "mic-off",
  <>
    {MIC}
    <path d="M4 4l16 16" />
  </>,
);

export const Volume2 = VoiceGlyph;
export const Headphones = AudioGlyph;
export const PhoneCall = CallGlyph;
export const ShieldCheck = PrivacyGlyph;
export const SlidersHorizontal = AdvancedGlyph;
export const Settings = SettingsGlyph;
export const Bug = DebugGlyph;
export const BookA = DictionaryGlyph;
export const Box = ModelsGlyph;
export const MessageCircleQuestionMark = HelpGlyph;

export const Trash2 = glyph(
  "trash",
  <>
    {shape(TRASH_BODY)}
    <path d={TRASH_LID} />
    <path d="M10 10.75v5.75M14 10.75v5.75" />
  </>,
);

export const Trash = glyph(
  "trash-plain",
  <>
    {shape(TRASH_BODY)}
    <path d={TRASH_LID} />
  </>,
);

export const Search = glyph(
  "search",
  <>
    <circle {...SOFT} cx="10.75" cy="10.75" r="6.5" />
    <circle cx="10.75" cy="10.75" r="6.5" />
    <path d="M15.6 15.6 20 20" />
  </>,
);

export const SearchCode = glyph(
  "search-code",
  <>
    <circle {...SOFT} cx="10.75" cy="10.75" r="6.5" />
    <circle cx="10.75" cy="10.75" r="6.5" />
    <path d="M15.6 15.6 20 20M9.5 8.75 7.75 10.75l1.75 2M12 8.75l1.75 2-1.75 2" />
  </>,
);

export const Pencil = glyph(
  "pencil",
  <>
    {shape(
      "M15.4 4.6a2.2 2.2 0 0 1 3.1 0l.9.9a2.2 2.2 0 0 1 0 3.1L9.1 18.9l-4.6 1 1-4.6z",
    )}
    <path d="M13.75 6.25l4 4" />
  </>,
);

export const SquarePen = glyph(
  "square-pen",
  <>
    <path d="M11 4.25H7a2.75 2.75 0 0 0-2.75 2.75v10A2.75 2.75 0 0 0 7 19.75h10A2.75 2.75 0 0 0 19.75 17v-4" />
    {shape("M17.1 3.9a1.95 1.95 0 0 1 2.75 2.75l-7.1 7.1-3.5.75.75-3.5z")}
  </>,
);

export const Eraser = glyph(
  "eraser",
  <>
    {shape(
      "M9.6 20 4.45 14.85a1.9 1.9 0 0 1 0-2.7l8.2-8.2a1.9 1.9 0 0 1 2.7 0l4.4 4.4a1.9 1.9 0 0 1 0 2.7L12.8 20z",
    )}
    <path d="M8.5 9.75l5.75 5.75M12.8 20h7" />
  </>,
);

export const Wand2 = glyph(
  "wand",
  <>
    <path d="M4.5 19.5 13 11" />
    {shape(SPARK(16.5, 7.5, 4))}
    <circle {...SOLID} cx="8.5" cy="5.5" r="1" />
    <circle {...SOLID} cx="19" cy="15" r="1" />
  </>,
);

export const Sparkles = glyph(
  "sparkles",
  <>
    {shape(SPARK(10, 13.5, 6.25))}
    <path d={SPARK(18, 5.75, 2.5)} />
    <circle {...SOLID} cx="18.5" cy="18" r="1" />
  </>,
);

export const Users = glyph(
  "users",
  <>
    <circle {...SOFT} cx="9" cy="8.5" r="3.5" />
    <circle cx="9" cy="8.5" r="3.5" />
    <path d="M3 19.75a6 6 0 0 1 12 0" />
    <path d="M15.75 5.25a3.25 3.25 0 0 1 0 6.5M17.75 14.1a6 6 0 0 1 3.25 5.65" />
  </>,
);

export const Speech = glyph(
  "speech",
  <>
    <circle {...SOFT} cx="9" cy="8.75" r="4" />
    <circle cx="9" cy="8.75" r="4" />
    <path d="M3.25 20a5.75 5.75 0 0 1 11.5 0" />
    <path d="M16.75 6.5a4 4 0 0 1 0 5M19.25 4.25a7.25 7.25 0 0 1 0 9.5" />
  </>,
);

export const MessageCircle = glyph("message-circle", shape(BUBBLE));

export const MessageSquareText = glyph(
  "message-square-text",
  <>
    {shape(
      "M4 6.75A2.75 2.75 0 0 1 6.75 4h10.5A2.75 2.75 0 0 1 20 6.75v7.5A2.75 2.75 0 0 1 17.25 17H10.5L6 20.25V17h-.25A1.75 1.75 0 0 1 4 15.25z",
    )}
    <path d="M8 8.75h8M8 12.25h5" />
  </>,
);

export const Cpu = glyph(
  "cpu",
  <>
    <rect {...SOFT} x="6" y="6" width="12" height="12" rx="3" />
    <rect x="6" y="6" width="12" height="12" rx="3" />
    <rect x="9.5" y="9.5" width="5" height="5" rx="1.25" />
    <path d="M9.75 3.5V6M14.25 3.5V6M9.75 18v2.5M14.25 18v2.5M3.5 9.75H6M3.5 14.25H6M18 9.75h2.5M18 14.25h2.5" />
  </>,
);

export const Copy = glyph(
  "copy",
  <>
    <path d="M15.25 8.5V6.25a2.5 2.5 0 0 0-2.5-2.5h-6.5a2.5 2.5 0 0 0-2.5 2.5v6.5a2.5 2.5 0 0 0 2.5 2.5H8.5" />
    <rect {...SOFT} x="8.5" y="8.5" width="11.75" height="11.75" rx="2.5" />
    <rect x="8.5" y="8.5" width="11.75" height="11.75" rx="2.5" />
  </>,
);

export const Play = glyph("play", shape(PLAY));
export const PlayIcon = Play;

export const Pause = glyph(
  "pause",
  <>
    <rect {...SOFT} x="6.25" y="5" width="4" height="14" rx="1.6" />
    <rect x="6.25" y="5" width="4" height="14" rx="1.6" />
    <rect {...SOFT} x="13.75" y="5" width="4" height="14" rx="1.6" />
    <rect x="13.75" y="5" width="4" height="14" rx="1.6" />
  </>,
);

export const HardDrive = glyph(
  "hard-drive",
  <>
    <rect {...SOFT} x="3" y="12.5" width="18" height="7" rx="2.5" />
    <rect x="3" y="12.5" width="18" height="7" rx="2.5" />
    <path d="M3.4 13.6 6 6.4A2.2 2.2 0 0 1 8.05 5h7.9A2.2 2.2 0 0 1 18 6.4l2.6 7.2M6.75 16h4.5" />
    <circle {...SOLID} cx="17" cy="16" r="1.05" />
  </>,
);

export const FolderOpen = glyph(
  "folder-open",
  <>
    <path d="M4 17.75V6.5a2 2 0 0 1 2-2h3.1a1.5 1.5 0 0 1 1.1.5l1.6 1.75H17a2 2 0 0 1 2 2V10" />
    {shape(
      "M4 17.75 6.3 11.3A2 2 0 0 1 8.2 10h11.9a1.25 1.25 0 0 1 1.2 1.6l-1.85 6.4a2 2 0 0 1-1.9 1.5H5.6A1.6 1.6 0 0 1 4 17.75z",
    )}
  </>,
);

export const Cloud = glyph(
  "cloud",
  shape(
    "M7.25 19h10.25a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.6 9.6 4.75 4.75 0 0 0 7.25 19z",
  ),
);

export const Smartphone = glyph(
  "smartphone",
  <>
    <rect {...SOFT} x="6.25" y="2.75" width="11.5" height="18.5" rx="3" />
    <rect x="6.25" y="2.75" width="11.5" height="18.5" rx="3" />
    <path d="M10.75 17.75h2.5" />
  </>,
);

export const Keyboard = glyph(
  "keyboard",
  <>
    <rect {...SOFT} x="2.75" y="5.75" width="18.5" height="12.5" rx="3" />
    <rect x="2.75" y="5.75" width="18.5" height="12.5" rx="3" />
    <circle {...SOLID} cx="7" cy="10" r="1" />
    <circle {...SOLID} cx="10.33" cy="10" r="1" />
    <circle {...SOLID} cx="13.67" cy="10" r="1" />
    <circle {...SOLID} cx="17" cy="10" r="1" />
    <path d="M8.5 14.25h7" />
  </>,
);

export const FileText = glyph(
  "file-text",
  <>
    {shape(DOC)}
    <path d={DOC_FOLD} />
    <path d="M9 12.5h6M9 16h4" />
  </>,
);

export const FilePlus2 = glyph(
  "file-plus",
  <>
    {shape(DOC)}
    <path d={DOC_FOLD} />
    <path d="M12 11.5v5.5M9.25 14.25h5.5" />
  </>,
);

export const FileQuestion = glyph(
  "file-question",
  <>
    {shape(DOC)}
    <path d={DOC_FOLD} />
    <path d="M10.1 11.6a1.95 1.95 0 1 1 2.65 1.8c-.45.2-.75.6-.75 1.05v.3" />
    <circle {...SOLID} cx="12" cy="17.25" r="1" />
  </>,
);

export const Eye = glyph(
  "eye",
  <>
    {shape(EYE)}
    <circle cx="12" cy="12" r="3" />
  </>,
);

export const EyeOff = glyph(
  "eye-off",
  <>
    {shape(EYE)}
    <circle cx="12" cy="12" r="3" />
    <path d="M4 4l16 16" />
  </>,
);

export const Plug = glyph(
  "plug",
  <>
    {shape("M7 9h10v3a5 5 0 0 1-10 0z")}
    <path d="M9.5 4.5V9M14.5 4.5V9M12 17v3.5" />
  </>,
);

export const PlugZap = glyph(
  "plug-zap",
  <>
    {shape("M4.5 9H14v3a4.75 4.75 0 0 1-9.5 0z")}
    <path d="M7 4.5V9M11.5 4.5V9M9.25 16.75v3.75" />
    <path d="M19 4.5 16.75 9h3.5L18 13.5" />
  </>,
);

export const Lightbulb = glyph(
  "lightbulb",
  <>
    {shape(
      "M9 16.25c0-1.5-.6-2.4-1.6-3.4a5.75 5.75 0 1 1 9.2 0c-1 1-1.6 1.9-1.6 3.4z",
    )}
    <path d="M9.5 19h5M10.5 21.25h3" />
  </>,
);

export const TriangleAlert = glyph(
  "triangle-alert",
  <>
    {shape(
      "M10.3 4.4a2 2 0 0 1 3.4 0l7.1 12.4a2 2 0 0 1-1.7 3H4.9a2 2 0 0 1-1.7-3z",
    )}
    <path d="M12 9.5v4" />
    <circle {...SOLID} cx="12" cy="16.6" r="1.05" />
  </>,
);
export const AlertTriangle = TriangleAlert;

export const Zap = glyph(
  "zap",
  shape("M13.5 3 5.5 13.25h6L10.5 21l8-10.25h-6z"),
);

export const Wrench = glyph(
  "wrench",
  shape(
    "M14.7 4.1a4.5 4.5 0 0 0-4.4 6.1L4.6 15.9a2.1 2.1 0 0 0 3 3l5.7-5.7a4.5 4.5 0 0 0 6.1-4.4L16.6 9.1l-1.5-1.5z",
  ),
);

export const Sun = glyph(
  "sun",
  <>
    <circle {...SOFT} cx="12" cy="12" r="4" />
    <circle cx="12" cy="12" r="4" />
    <path d="M12 3v1.75M12 19.25V21M3 12h1.75M19.25 12H21M5.65 5.65l1.25 1.25M17.1 17.1l1.25 1.25M5.65 18.35l1.25-1.25M17.1 6.9l1.25-1.25" />
  </>,
);

export const Moon = glyph(
  "moon",
  shape("M19.5 14.25A7.75 7.75 0 0 1 9.75 4.5a7.75 7.75 0 1 0 9.75 9.75z"),
);

export const SunMoon = glyph(
  "sun-moon",
  <>
    <circle cx="12" cy="12" r="4.5" />
    <path {...SOLID} d="M12 7.5a4.5 4.5 0 0 1 0 9z" />
    <path d="M12 3v1.75M12 19.25V21M3 12h1.75M5.65 5.65l1.25 1.25M5.65 18.35l1.25-1.25" />
  </>,
);

export const Star = glyph(
  "star",
  shape(
    "M12 3.85l2.47 5.35 5.85.7-4.33 4 1.15 5.78L12 16.8l-5.14 2.88 1.15-5.78-4.33-4 5.85-.7z",
  ),
);

export const SmilePlus = glyph(
  "smile-plus",
  <>
    <circle {...SOFT} cx="11" cy="13" r="7.5" />
    <path d="M18.4 11.7A7.5 7.5 0 1 1 12.3 5.6" />
    <circle {...SOLID} cx="8.5" cy="11.75" r="1" />
    <circle {...SOLID} cx="13.5" cy="11.75" r="1" />
    <path d="M8.25 15.25a3.4 3.4 0 0 0 5.5 0M18.25 3.25v5M15.75 5.75h5" />
  </>,
);

export const Share2 = glyph(
  "share",
  <>
    <circle {...SOFT} cx="17" cy="5.75" r="2.6" />
    <circle cx="17" cy="5.75" r="2.6" />
    <circle {...SOFT} cx="6.75" cy="12" r="2.6" />
    <circle cx="6.75" cy="12" r="2.6" />
    <circle {...SOFT} cx="17" cy="18.25" r="2.6" />
    <circle cx="17" cy="18.25" r="2.6" />
    <path d="M9 10.65l5.75-3.55M9 13.35l5.75 3.55" />
  </>,
);

export const Scale = glyph(
  "scale",
  <>
    <path d="M12 4v16M8 20h8M5 7.5h14" />
    {shape("M2.75 14.25 5 7.5l2.25 6.75a2.6 2.6 0 0 1-4.5 0z")}
    {shape("M16.75 14.25 19 7.5l2.25 6.75a2.6 2.6 0 0 1-4.5 0z")}
  </>,
);

export const RectangleVertical = glyph(
  "rectangle-vertical",
  <>
    <rect {...SOFT} x="6.5" y="3.5" width="11" height="17" rx="3" />
    <rect x="6.5" y="3.5" width="11" height="17" rx="3" />
  </>,
);

export const RectangleHorizontal = glyph(
  "rectangle-horizontal",
  <>
    <rect {...SOFT} x="3.5" y="6.5" width="17" height="11" rx="3" />
    <rect x="3.5" y="6.5" width="17" height="11" rx="3" />
  </>,
);

export const PanelTop = glyph(
  "panel-top",
  <>
    <rect {...SOFT} x="3.25" y="4" width="17.5" height="16" rx="3.5" />
    <rect x="3.25" y="4" width="17.5" height="16" rx="3.5" />
    <path d="M3.25 9.5h17.5" />
  </>,
);

export const Monitor = glyph(
  "monitor",
  <>
    <rect {...SOFT} x="2.75" y="3.75" width="18.5" height="12.5" rx="2.75" />
    <rect x="2.75" y="3.75" width="18.5" height="12.5" rx="2.75" />
    <path d="M12 16.25v3.75M8.75 20.25h6.5" />
  </>,
);

export const MemoryStick = glyph(
  "memory-stick",
  <>
    <rect {...SOFT} x="3" y="6.5" width="18" height="10" rx="2.25" />
    <rect x="3" y="6.5" width="18" height="10" rx="2.25" />
    <path d="M7 10v3M10.33 10v3M13.67 10v3M17 10v3M6.5 16.5v2.75M9.5 16.5v2.75M14.5 16.5v2.75M17.5 16.5v2.75" />
  </>,
);

export const ImagePlus = glyph(
  "image-plus",
  <>
    <rect {...SOFT} x="3.5" y="3.5" width="17" height="17" rx="3" />
    <path d="M12.5 3.5h-6a3 3 0 0 0-3 3v11a3 3 0 0 0 3 3h11a3 3 0 0 0 3-3v-6" />
    <circle {...SOLID} cx="8.75" cy="9" r="1.5" />
    <path d="M3.75 17.25l4.6-4.6a1.5 1.5 0 0 1 2.1 0l6.3 6.3M14 15.75l1.35-1.35a1.5 1.5 0 0 1 2.1 0l3 3M18 2.75v5.5M15.25 5.5h5.5" />
  </>,
);

export const House = glyph(
  "house",
  <>
    {shape(
      "M4 10.4 12 4l8 6.4v8.6a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 19z",
    )}
    <path d="M9.75 20.5v-4.75a1 1 0 0 1 1-1h2.5a1 1 0 0 1 1 1v4.75" />
  </>,
);

export const Heart = glyph(
  "heart",
  shape(
    "M12 20s-8-4.7-8-10.5A4.5 4.5 0 0 1 12 6.8 4.5 4.5 0 0 1 20 9.5C20 15.3 12 20 12 20z",
  ),
);

export const GraduationCap = glyph(
  "graduation-cap",
  <>
    {shape("M2.75 9.5 12 5l9.25 4.5L12 14z")}
    <path d="M6.5 11.5v4.25c1.5 1.5 3.4 2.25 5.5 2.25s4-.75 5.5-2.25V11.5M21.25 9.5v5" />
  </>,
);

export const Github = glyph(
  "github",
  <>
    {shape(
      "M9.5 20.25v-2.6c0-.8.2-1.4.6-1.9-2.6-.3-5.1-1.3-5.1-5.6 0-1.2.4-2.3 1.2-3.1-.2-.6-.4-1.8.2-3.3 0 0 1-.3 3.2 1.2a10.6 10.6 0 0 1 5.6 0c2.2-1.5 3.2-1.2 3.2-1.2.6 1.5.4 2.7.2 3.3.8.8 1.2 1.9 1.2 3.1 0 4.3-2.6 5.3-5.1 5.6.4.4.7 1.1.7 2.1v2.4",
    )}
    <path d="M9.5 18.25c-2.6.8-3.5-.7-4.6-1.9" />
  </>,
);

export const FlaskConical = glyph(
  "flask",
  <>
    <path
      {...SOFT}
      d="M7.1 14.5h9.8l2.1 3.7a1.6 1.6 0 0 1-1.4 2.3H6.4a1.6 1.6 0 0 1-1.4-2.3z"
    />
    <path d="M9.25 3.5h5.5M10.25 3.5V9L5 18.2a1.6 1.6 0 0 0 1.4 2.3h11.2a1.6 1.6 0 0 0 1.4-2.3L13.75 9V3.5M7.1 14.5h9.8" />
  </>,
);

export const Flame = glyph(
  "flame",
  <>
    {shape(
      "M12 21a6.5 6.5 0 0 0 6.5-6.5c0-3.9-3-6.2-4.5-10.5-1 2.5-2.5 3.6-3.5 4.4C9.8 7.2 8.6 6.2 8.2 5.4 6.4 7.6 5.5 10 5.5 14.5A6.5 6.5 0 0 0 12 21z",
    )}
    <path d="M12 18a2.3 2.3 0 0 1-2.3-2.3c0-1.5 1.3-2.4 2.3-4 1 1.6 2.3 2.5 2.3 4A2.3 2.3 0 0 1 12 18z" />
  </>,
);

export const Fish = glyph(
  "fish",
  <>
    {shape(
      "M20.5 12c-1.9 3.4-4.4 5.25-7.5 5.25-2.4 0-4.4-1.1-6-3L3.5 16.5v-9L7 9.75C8.6 7.85 10.6 6.75 13 6.75c3.1 0 5.6 1.85 7.5 5.25z",
    )}
    <circle {...SOLID} cx="16.25" cy="10.75" r="1.05" />
  </>,
);

export const ChartColumn = glyph(
  "chart-column",
  <>
    <path d="M3.75 3.75v14.5a2 2 0 0 0 2 2h14.5" />
    <rect {...SOFT} x="7.5" y="12" width="3" height="5" rx="1" />
    <rect x="7.5" y="12" width="3" height="5" rx="1" />
    <rect {...SOFT} x="12" y="6.75" width="3" height="10.25" rx="1" />
    <rect x="12" y="6.75" width="3" height="10.25" rx="1" />
    <rect {...SOFT} x="16.5" y="9.75" width="3" height="7.25" rx="1" />
    <rect x="16.5" y="9.75" width="3" height="7.25" rx="1" />
  </>,
);

export const Cat = glyph(
  "cat",
  <>
    {shape(
      "M5 9.5V4.5l3.5 3a8.5 8.5 0 0 1 7 0l3.5-3v5c.8 1.2 1.25 2.5 1.25 4 0 4-3.6 6.75-8.25 6.75S3.75 17.5 3.75 13.5c0-1.5.45-2.8 1.25-4z",
    )}
    <circle {...SOLID} cx="9" cy="13" r="1.05" />
    <circle {...SOLID} cx="15" cy="13" r="1.05" />
    <path d="M11.25 16.25h1.5" />
  </>,
);

export const Camera = glyph(
  "camera",
  <>
    {shape(
      "M3.25 9a2.5 2.5 0 0 1 2.5-2.5h1.9l1.4-2a1.5 1.5 0 0 1 1.25-.65h3.4a1.5 1.5 0 0 1 1.25.65l1.4 2h1.9A2.5 2.5 0 0 1 20.75 9v8.5a2.5 2.5 0 0 1-2.5 2.5H5.75a2.5 2.5 0 0 1-2.5-2.5z",
    )}
    <circle cx="12" cy="13" r="3.5" />
  </>,
);

export const Calendar = glyph("calendar", CALENDAR);

export const CalendarDays = glyph(
  "calendar-days",
  <>
    {CALENDAR}
    <circle {...SOLID} cx="8" cy="13.75" r="1" />
    <circle {...SOLID} cx="12" cy="13.75" r="1" />
    <circle {...SOLID} cx="16" cy="13.75" r="1" />
    <circle {...SOLID} cx="8" cy="17" r="1" />
    <circle {...SOLID} cx="12" cy="17" r="1" />
  </>,
);

export const Bell = glyph(
  "bell",
  <>
    {shape(BELL)}
    <path d="M10 19.25a2.2 2.2 0 0 0 4 0" />
  </>,
);

export const BellRing = glyph(
  "bell-ring",
  <>
    {shape(BELL)}
    <path d="M10 19.25a2.2 2.2 0 0 0 4 0M3.25 7A8 8 0 0 1 5.5 3.75M20.75 7a8 8 0 0 0-2.25-3.25" />
  </>,
);

export const Mail = glyph(
  "mail",
  <>
    <rect {...SOFT} x="3" y="5.25" width="18" height="13.5" rx="3" />
    <rect x="3" y="5.25" width="18" height="13.5" rx="3" />
    <path d="M3.75 8l7.05 4.85a2.1 2.1 0 0 0 2.4 0L20.25 8" />
  </>,
);

export const Lock = glyph(
  "lock",
  <>
    <rect {...SOFT} x="4.75" y="10.25" width="14.5" height="10.25" rx="2.75" />
    <rect x="4.75" y="10.25" width="14.5" height="10.25" rx="2.75" />
    <path d="M8 10.25V8a4 4 0 0 1 8 0v2.25M12 14.25v2.25" />
  </>,
);

export const Filter = glyph(
  "filter",
  shape("M4 5h16l-6.25 7.5v5.25l-3.5 2.25v-7.5z"),
);

export const Grid = glyph(
  "grid",
  <>
    <rect {...SOFT} x="3.75" y="3.75" width="7" height="7" rx="2" />
    <rect x="3.75" y="3.75" width="7" height="7" rx="2" />
    <rect {...SOFT} x="13.25" y="3.75" width="7" height="7" rx="2" />
    <rect x="13.25" y="3.75" width="7" height="7" rx="2" />
    <rect {...SOFT} x="3.75" y="13.25" width="7" height="7" rx="2" />
    <rect x="3.75" y="13.25" width="7" height="7" rx="2" />
    <rect {...SOFT} x="13.25" y="13.25" width="7" height="7" rx="2" />
    <rect x="13.25" y="13.25" width="7" height="7" rx="2" />
  </>,
);
