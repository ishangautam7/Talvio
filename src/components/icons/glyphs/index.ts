/**
 * The main window's icons: Lucide (lucide-react, pinned to one exact
 * version in package.json), drawn at one stroke everywhere.
 *
 * Every icon the main window shows comes from this module, never from
 * lucide-react directly, so the whole set keeps one weight: a 1.75 stroke on
 * Lucide's 24 grid, which reads as a crisp 1.2–1.3 px line at the 16–18 px
 * the window draws icons at. A call site may still pass `strokeWidth` (a
 * check mark inside a small tick box wants a heavier line), but nothing else
 * changes how an icon is drawn.
 *
 * The feature names (`HomeGlyph`, `MeetingsGlyph`, …) are the one place a
 * page or job is given its icon, so the navigation, the palette and the
 * Settings tabs cannot disagree. The window's earlier hand-drawn set (Talvio
 * Glyphs) was replaced because its shapes and optical weights were uneven
 * beside each other; the floating windows keep their own icons.
 */
import { createElement, forwardRef } from "react";
import * as L from "lucide-react";

export type GlyphProps = L.LucideProps;
export type Glyph = L.LucideIcon;
/** lucide-react's icon type, for call sites that still name it. */
export type LucideIcon = L.LucideIcon;

/** The one stroke every icon in the main window is drawn with. */
export const GLYPH_STROKE = 1.75;

/** "AudioLines" → "audio-lines", for the `glyph-*` class each icon carries. */
const kebab = (name: string) =>
  name.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase();

const wrap = (Icon: L.LucideIcon, name: string): Glyph => {
  const base = `glyph glyph-${kebab(name)}`;
  const Component = forwardRef<SVGSVGElement, GlyphProps>((props, ref) =>
    createElement(Icon, {
      strokeWidth: GLYPH_STROKE,
      "aria-hidden": props["aria-label"] ? undefined : true,
      focusable: "false",
      ...props,
      className: props.className ? `${base} ${props.className}` : base,
      ref,
    }),
  );
  Component.displayName = `Glyph(${name})`;
  return Component as Glyph;
};

/* ---------------------------------------------------------------- features */

export const HomeGlyph = wrap(L.House, "Home");
export const InsightsGlyph = wrap(L.ChartNoAxesColumn, "Insights");
export const HistoryGlyph = wrap(L.History, "History");
export const CleanupGlyph = wrap(L.WandSparkles, "Cleanup");
export const DictionaryGlyph = wrap(L.BookA, "Dictionary");
export const AssistantGlyph = wrap(L.Sparkles, "Assistant");
export const CommandsGlyph = wrap(L.SquareTerminal, "Commands");
export const MeetingsGlyph = wrap(L.Users, "Meetings");
export const ModelsGlyph = wrap(L.Box, "Models");
export const SettingsGlyph = wrap(L.Settings, "Settings");
export const HelpGlyph = wrap(L.CircleHelp, "Help");
export const DictateGlyph = wrap(L.Mic, "Dictate");
export const CallGlyph = wrap(L.PhoneCall, "Call");
export const AccountGlyph = wrap(L.CircleUserRound, "Account");
export const SpeechToTextGlyph = wrap(L.AudioLines, "SpeechToText");
export const VoiceGlyph = wrap(L.Volume2, "Voice");
export const ShortcutsGlyph = wrap(L.Keyboard, "Shortcuts");
export const AudioGlyph = wrap(L.Headphones, "Audio");
export const PrivacyGlyph = wrap(L.ShieldCheck, "Privacy");
export const AdvancedGlyph = wrap(L.SlidersHorizontal, "Advanced");
export const AboutGlyph = wrap(L.Info, "About");
export const DebugGlyph = wrap(L.Bug, "Debug");
export const TypingGlyph = wrap(L.TextCursorInput, "Typing");

/* ---------------------------------------------------------------- interface */

export const Check = wrap(L.Check, "Check");
export const X = wrap(L.X, "X");
export const Plus = wrap(L.Plus, "Plus");
export const Minus = wrap(L.Minus, "Minus");
export const ChevronDown = wrap(L.ChevronDown, "ChevronDown");
export const ChevronUp = wrap(L.ChevronUp, "ChevronUp");
export const ChevronRight = wrap(L.ChevronRight, "ChevronRight");
export const ChevronLeft = wrap(L.ChevronLeft, "ChevronLeft");
export const ChevronsUpDown = wrap(L.ChevronsUpDown, "ChevronsUpDown");
export const ArrowRight = wrap(L.ArrowRight, "ArrowRight");
export const ArrowLeft = wrap(L.ArrowLeft, "ArrowLeft");
export const ArrowUp = wrap(L.ArrowUp, "ArrowUp");
export const ArrowUpRight = wrap(L.ArrowUpRight, "ArrowUpRight");
export const CornerDownLeft = wrap(L.CornerDownLeft, "CornerDownLeft");
export const ExternalLink = wrap(L.ExternalLink, "ExternalLink");
export const MoreHorizontal = wrap(L.Ellipsis, "MoreHorizontal");
export const Ellipsis = MoreHorizontal;
export const Loader2 = wrap(L.LoaderCircle, "Loader2");
export const LoaderCircle = Loader2;
export const Loader = wrap(L.Loader, "Loader");
export const RefreshCw = wrap(L.RefreshCw, "RefreshCw");
export const RotateCw = wrap(L.RotateCw, "RotateCw");
export const RotateCcw = wrap(L.RotateCcw, "RotateCcw");
export const Download = wrap(L.Download, "Download");
export const Upload = wrap(L.Upload, "Upload");
export const Square = wrap(L.Square, "Square");
export const Type = wrap(L.Type, "Type");
export const List = wrap(L.List, "List");
export const AudioLines = wrap(L.AudioLines, "AudioLines");
export const AudioWaveform = wrap(L.AudioWaveform, "AudioWaveform");
export const Waves = wrap(L.Waves, "Waves");
export const Languages = wrap(L.Languages, "Languages");
export const GitBranch = wrap(L.GitBranch, "GitBranch");
export const Link = wrap(L.Link, "Link");
export const LogOut = wrap(L.LogOut, "LogOut");
export const TextSelect = wrap(L.TextSelect, "TextSelect");
export const TextCursorInput = wrap(L.TextCursorInput, "TextCursorInput");
export const Info = wrap(L.Info, "Info");
export const CircleQuestionMark = wrap(L.CircleHelp, "CircleQuestionMark");
export const HelpCircle = CircleQuestionMark;
export const CircleCheck = wrap(L.CircleCheck, "CircleCheck");
export const CheckCircle = CircleCheck;
export const CircleAlert = wrap(L.CircleAlert, "CircleAlert");
export const AlertCircle = CircleAlert;
export const CircleArrowUp = wrap(L.CircleArrowUp, "CircleArrowUp");
export const CircleUserRound = wrap(L.CircleUserRound, "CircleUserRound");
export const Clock = wrap(L.Clock, "Clock");
export const History = wrap(L.History, "History");
export const Globe = wrap(L.Globe, "Globe");
// Drawn as the old set drew it: one globe, under both names.
export const Earth = wrap(L.Earth, "Globe");
export const Power = wrap(L.Power, "Power");
export const Gauge = wrap(L.Gauge, "Gauge");
export const Radio = wrap(L.Radio, "Radio");
export const Mic = wrap(L.Mic, "Mic");
export const MicOff = wrap(L.MicOff, "MicOff");
export const Volume2 = wrap(L.Volume2, "Volume2");
export const Headphones = wrap(L.Headphones, "Headphones");
export const PhoneCall = wrap(L.PhoneCall, "PhoneCall");
export const ShieldCheck = wrap(L.ShieldCheck, "ShieldCheck");
export const SlidersHorizontal = wrap(L.SlidersHorizontal, "SlidersHorizontal");
export const Settings = wrap(L.Settings, "Settings");
export const Bug = wrap(L.Bug, "Bug");
export const BookA = wrap(L.BookA, "BookA");
export const Box = wrap(L.Box, "Box");
export const MessageCircleQuestionMark = wrap(
  L.MessageCircleQuestion,
  "MessageCircleQuestionMark",
);
export const Trash2 = wrap(L.Trash2, "Trash2");
export const Trash = wrap(L.Trash, "Trash");
export const Search = wrap(L.Search, "Search");
export const SearchCode = wrap(L.SearchCode, "SearchCode");
export const Pencil = wrap(L.Pencil, "Pencil");
export const SquarePen = wrap(L.SquarePen, "SquarePen");
export const Eraser = wrap(L.Eraser, "Eraser");
export const Wand2 = wrap(L.WandSparkles, "Wand2");
export const Sparkles = wrap(L.Sparkles, "Sparkles");
export const Gift = wrap(L.Gift, "Gift");
export const Users = wrap(L.Users, "Users");
export const Speech = wrap(L.Speech, "Speech");
export const MessageCircle = wrap(L.MessageCircle, "MessageCircle");
export const MessageSquareText = wrap(L.MessageSquareText, "MessageSquareText");
export const Cpu = wrap(L.Cpu, "Cpu");
export const Copy = wrap(L.Copy, "Copy");
export const Play = wrap(L.Play, "Play");
export const PlayIcon = Play;
export const Pause = wrap(L.Pause, "Pause");
export const HardDrive = wrap(L.HardDrive, "HardDrive");
export const FolderOpen = wrap(L.FolderOpen, "FolderOpen");
export const Cloud = wrap(L.Cloud, "Cloud");
export const Smartphone = wrap(L.Smartphone, "Smartphone");
export const Keyboard = wrap(L.Keyboard, "Keyboard");
export const FileText = wrap(L.FileText, "FileText");
export const FilePlus2 = wrap(L.FilePlus2, "FilePlus2");
export const FileQuestion = wrap(L.FileQuestion, "FileQuestion");
export const Eye = wrap(L.Eye, "Eye");
export const EyeOff = wrap(L.EyeOff, "EyeOff");
export const Plug = wrap(L.Plug, "Plug");
export const PlugZap = wrap(L.PlugZap, "PlugZap");
export const Lightbulb = wrap(L.Lightbulb, "Lightbulb");
export const TriangleAlert = wrap(L.TriangleAlert, "TriangleAlert");
export const AlertTriangle = TriangleAlert;
export const Zap = wrap(L.Zap, "Zap");
export const Wrench = wrap(L.Wrench, "Wrench");
export const Sun = wrap(L.Sun, "Sun");
export const Moon = wrap(L.Moon, "Moon");
export const SunMoon = wrap(L.SunMoon, "SunMoon");
export const Star = wrap(L.Star, "Star");
export const SmilePlus = wrap(L.SmilePlus, "SmilePlus");
export const Share2 = wrap(L.Share2, "Share2");
export const Scale = wrap(L.Scale, "Scale");
export const RectangleVertical = wrap(L.RectangleVertical, "RectangleVertical");
export const RectangleHorizontal = wrap(
  L.RectangleHorizontal,
  "RectangleHorizontal",
);
export const PanelTop = wrap(L.PanelTop, "PanelTop");
export const Monitor = wrap(L.Monitor, "Monitor");
export const MemoryStick = wrap(L.MemoryStick, "MemoryStick");
export const ImagePlus = wrap(L.ImagePlus, "ImagePlus");
export const House = wrap(L.House, "House");
export const Heart = wrap(L.Heart, "Heart");
export const GraduationCap = wrap(L.GraduationCap, "GraduationCap");
export const Github = wrap(L.Github, "Github");
export const FlaskConical = wrap(L.FlaskConical, "FlaskConical");
export const Flame = wrap(L.Flame, "Flame");
export const Fish = wrap(L.Fish, "Fish");
export const ChartColumn = wrap(L.ChartColumn, "ChartColumn");
export const Cat = wrap(L.Cat, "Cat");
export const Camera = wrap(L.Camera, "Camera");
export const Calendar = wrap(L.Calendar, "Calendar");
export const CalendarDays = wrap(L.CalendarDays, "CalendarDays");
export const Bell = wrap(L.Bell, "Bell");
export const BellRing = wrap(L.BellRing, "BellRing");
export const Mail = wrap(L.Mail, "Mail");
export const Lock = wrap(L.Lock, "Lock");
export const Filter = wrap(L.Funnel, "Filter");
export const Grid = wrap(L.Grid3x3, "Grid");
