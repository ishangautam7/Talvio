import { glyph, SOFT, SOLID } from "./base";

/**
 * The glyph of each page and feature. Each says what its page is about, and
 * no two share a silhouette: Meetings is two voices on two lanes, the
 * assistant an orb with a spark, History a list with a clock, Commands a
 * pointer that clicks.
 */

/** Home: a house with the logo's waveform for a door. */
export const HomeGlyph = glyph(
  "home",
  <>
    <path
      {...SOFT}
      d="M4 10.4 12 4l8 6.4v8.6a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 19z"
    />
    <path d="M4 10.4 12 4l8 6.4v8.6a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 19z" />
    <path d="M9.4 14.6v1.8M12 12.8v5.4M14.6 14.6v1.8" />
  </>,
);

/** History: what you said, listed, with the time it was said. */
export const HistoryGlyph = glyph(
  "history",
  <>
    <path d="M3.75 5.5h12M3.75 10h7.5M3.75 14.5h4" />
    <circle {...SOFT} cx="16" cy="16" r="5" />
    <circle cx="16" cy="16" r="5" />
    <path d="M16 13.75V16l1.6 1" />
  </>,
);

/** The assistant: an orb with a spark in it. */
export const AssistantGlyph = glyph(
  "assistant",
  <>
    <circle {...SOFT} cx="12" cy="12" r="8.75" />
    <circle cx="12" cy="12" r="8.75" />
    <path
      {...SOLID}
      d="M12 7.6c.35 2.25 1.9 3.8 4.15 4.15C13.9 12.1 12.35 13.65 12 15.9c-.35-2.25-1.9-3.8-4.15-4.15C10.1 11.4 11.65 9.85 12 7.6z"
    />
  </>,
);

/** Commands: a pointer that clicks, because a command does things. */
export const CommandsGlyph = glyph(
  "commands",
  <>
    <path {...SOFT} d="M9 9l11 4.3-4.9 1.6-1.6 4.9z" />
    <path d="M9 9l11 4.3-4.9 1.6-1.6 4.9z" />
    <path d="M9 4.5V3M4.5 9H3M5.8 5.8 4.6 4.6M12.6 5.2l1-1M5.2 12.6l-1 1" />
  </>,
);

/** Meetings: two voices, one lane each, taking turns. */
export const MeetingsGlyph = glyph(
  "meetings",
  <>
    <rect {...SOFT} x="2.75" y="4" width="18.5" height="16" rx="4.25" />
    <rect x="2.75" y="4" width="18.5" height="16" rx="4.25" />
    <path d="M6.5 9.25v.5M8.75 8v3M11 9v1M13.25 9.25v.5" />
    <path d="M10.75 14.25v.5M13 13v3M15.25 13.75v1.5M17.5 14.25v.5" />
  </>,
);

/** AI cleanup: a messy line becomes straight ones, with a spark. */
export const CleanupGlyph = glyph(
  "cleanup",
  <>
    <path d="M3.5 6.5c1.4-1.5 2.8-1.5 4.2 0s2.8 1.5 4.2 0" />
    <path d="M3.5 12h7.5M3.5 17.5h5" />
    <path
      {...SOFT}
      d="M17 8.25c.4 2.45 1.85 3.9 4.3 4.3-2.45.4-3.9 1.85-4.3 4.3-.4-2.45-1.85-3.9-4.3-4.3 2.45-.4 3.9-1.85 4.3-4.3z"
    />
    <path d="M17 8.25c.4 2.45 1.85 3.9 4.3 4.3-2.45.4-3.9 1.85-4.3 4.3-.4-2.45-1.85-3.9-4.3-4.3 2.45-.4 3.9-1.85 4.3-4.3z" />
  </>,
);

/** Dictionary: a book with a letter on its cover. */
export const DictionaryGlyph = glyph(
  "dictionary",
  <>
    <path {...SOFT} d="M5 5a2 2 0 0 1 2-2h12v14.5H7a2 2 0 0 0-2 2z" />
    <path d="M5 19.5V5a2 2 0 0 1 2-2h12v14.5H7a2 2 0 0 0 0 4h12" />
    <path d="M9.6 13.25 12 7.25l2.4 6M10.45 11.25h3.1" />
  </>,
);

/** Insights: bars that rise, and the line they make. */
export const InsightsGlyph = glyph(
  "insights",
  <>
    <rect {...SOFT} x="3.75" y="14.5" width="4" height="6" rx="1.25" />
    <rect {...SOFT} x="10" y="11.5" width="4" height="9" rx="1.25" />
    <rect {...SOFT} x="16.25" y="8.5" width="4" height="12" rx="1.25" />
    <path d="M3.75 20.5h16.5" />
    <path d="M4.5 10.5 9.5 7l4 2 6.5-5" />
    <path d="M16.5 4h3.5v3.5" />
  </>,
);

/** Models: a cube, the thing that does the work. */
export const ModelsGlyph = glyph(
  "models",
  <>
    <path {...SOFT} d="M12 3.25 19.75 7.6 12 12 4.25 7.6z" />
    <path d="M12 3.25 19.75 7.6v8.8L12 20.75 4.25 16.4V7.6z" />
    <path d="M4.25 7.6 12 12l7.75-4.4M12 12v8.75" />
  </>,
);

/** Settings: a gear with rounded teeth. */
export const SettingsGlyph = glyph(
  "settings",
  <>
    <path
      {...SOFT}
      d="M18.79 10.31 21.08 10.49v3.02l-2.29.18-.79 1.92 1.49 1.74-2.14 2.14-1.74-1.49-1.92.79-.18 2.29h-3.02l-.18-2.29-1.92-.79-1.74 1.49-2.14-2.14 1.49-1.74-.79-1.92-2.29-.18v-3.02l2.29-.18.79-1.92-1.49-1.74 2.14-2.14 1.74 1.49 1.92-.79.18-2.29h3.02l.18 2.29 1.92.79 1.74-1.49 2.14 2.14-1.49 1.74z"
    />
    <path d="M18.79 10.31 21.08 10.49v3.02l-2.29.18-.79 1.92 1.49 1.74-2.14 2.14-1.74-1.49-1.92.79-.18 2.29h-3.02l-.18-2.29-1.92-.79-1.74 1.49-2.14-2.14 1.49-1.74-.79-1.92-2.29-.18v-3.02l2.29-.18.79-1.92-1.49-1.74 2.14-2.14 1.74 1.49 1.92-.79.18-2.29h3.02l.18 2.29 1.92.79 1.74-1.49 2.14 2.14-1.49 1.74z" />
    <circle cx="12" cy="12" r="2.9" />
  </>,
);

/** Help and feedback: a question in a speech bubble. */
export const HelpGlyph = glyph(
  "help",
  <>
    <path
      {...SOFT}
      d="M12 3.5a8.5 8.5 0 0 1 0 17c-1.4 0-2.7-.34-3.86-.94L3.5 20.5l.94-4.64A8.5 8.5 0 0 1 12 3.5z"
    />
    <path d="M12 3.5a8.5 8.5 0 0 1 0 17c-1.4 0-2.7-.34-3.86-.94L3.5 20.5l.94-4.64A8.5 8.5 0 0 1 12 3.5z" />
    <path d="M9.75 9.75a2.3 2.3 0 1 1 3.1 2.15c-.55.22-.85.7-.85 1.25v.35" />
    <circle {...SOLID} cx="12" cy="16.4" r="1" />
  </>,
);

/** Dictation: a microphone with the waveform beside it. */
export const DictateGlyph = glyph(
  "dictate",
  <>
    <rect {...SOFT} x="8.25" y="2.75" width="7.5" height="12" rx="3.75" />
    <rect x="8.25" y="2.75" width="7.5" height="12" rx="3.75" />
    <path d="M5 11.5a7 7 0 0 0 14 0M12 18.5v2.75" />
  </>,
);

/** A call with the assistant: a handset inside a voice ring. */
export const CallGlyph = glyph(
  "call",
  <>
    <path
      {...SOFT}
      d="M8.1 4.5 9.6 8a1.2 1.2 0 0 1-.3 1.37l-1.2 1.08a9.2 9.2 0 0 0 5.45 5.45l1.08-1.2A1.2 1.2 0 0 1 16 14.4l3.5 1.5a1.2 1.2 0 0 1 .7 1.33l-.42 1.9A1.75 1.75 0 0 1 18.07 20.5C10.6 20.3 3.7 13.4 3.5 5.93A1.75 1.75 0 0 1 4.87 4.22l1.9-.42a1.2 1.2 0 0 1 1.33.7z"
    />
    <path d="M8.1 4.5 9.6 8a1.2 1.2 0 0 1-.3 1.37l-1.2 1.08a9.2 9.2 0 0 0 5.45 5.45l1.08-1.2A1.2 1.2 0 0 1 16 14.4l3.5 1.5a1.2 1.2 0 0 1 .7 1.33l-.42 1.9A1.75 1.75 0 0 1 18.07 20.5C10.6 20.3 3.7 13.4 3.5 5.93A1.75 1.75 0 0 1 4.87 4.22l1.9-.42a1.2 1.2 0 0 1 1.33.7z" />
    <path d="M14.5 3.75a6 6 0 0 1 5.75 5.75M14.25 7.25a2.75 2.75 0 0 1 2.5 2.5" />
  </>,
);

/** The account: a person on a card. */
export const AccountGlyph = glyph(
  "account",
  <>
    <circle {...SOFT} cx="12" cy="12" r="8.75" />
    <circle cx="12" cy="12" r="8.75" />
    <circle cx="12" cy="10" r="3" />
    <path d="M6.4 18.6a6.5 6.5 0 0 1 11.2 0" />
  </>,
);

/** Speech to text: the waveform becomes lines. */
export const SpeechToTextGlyph = glyph(
  "speech-to-text",
  <>
    <path d="M3.5 10.5v3M6.5 8v8M9.5 10v4" />
    <rect {...SOFT} x="12.5" y="5" width="8" height="14" rx="2.25" />
    <path d="M15 9.5h3M15 12.5h3M15 15.5h1.75" />
  </>,
);

/** A voice: a speaker with the sound leaving it. */
export const VoiceGlyph = glyph(
  "voice",
  <>
    <path {...SOFT} d="M4 9.5h2.75L11 5.75v12.5L6.75 14.5H4z" />
    <path d="M4 9.5h2.75L11 5.75v12.5L6.75 14.5H4z" />
    <path d="M14.75 9.25a3.9 3.9 0 0 1 0 5.5M17.5 6.75a7.4 7.4 0 0 1 0 10.5" />
  </>,
);

/** Shortcuts: two keys. */
export const ShortcutsGlyph = glyph(
  "shortcuts",
  <>
    <rect {...SOFT} x="2.75" y="7" width="8.5" height="10" rx="2.25" />
    <rect x="2.75" y="7" width="8.5" height="10" rx="2.25" />
    <rect x="12.75" y="7" width="8.5" height="10" rx="2.25" />
    <path d="M5.75 13.25h2.5M15.75 13.25h2.5" />
  </>,
);

/** Audio: a pair of headphones. */
export const AudioGlyph = glyph(
  "audio",
  <>
    <rect {...SOFT} x="3.75" y="12.75" width="3.5" height="6.75" rx="1.5" />
    <rect {...SOFT} x="16.75" y="12.75" width="3.5" height="6.75" rx="1.5" />
    <path d="M4 15v-1.5a8 8 0 0 1 16 0V15" />
    <rect x="3.75" y="12.75" width="3.5" height="6.75" rx="1.5" />
    <rect x="16.75" y="12.75" width="3.5" height="6.75" rx="1.5" />
  </>,
);

/** Privacy and data: a shield with a check. */
export const PrivacyGlyph = glyph(
  "privacy",
  <>
    <path
      {...SOFT}
      d="M12 3 19.5 5.75v5.5c0 4.6-3.1 8.2-7.5 9.75-4.4-1.55-7.5-5.15-7.5-9.75v-5.5z"
    />
    <path d="M12 3 19.5 5.75v5.5c0 4.6-3.1 8.2-7.5 9.75-4.4-1.55-7.5-5.15-7.5-9.75v-5.5z" />
    <path d="m9 12 2.1 2.1L15.25 10" />
  </>,
);

/** Advanced: two sliders. */
export const AdvancedGlyph = glyph(
  "advanced",
  <>
    <path d="M4 7.5h7M17 7.5h3M4 16.5h3M13 16.5h7" />
    <circle {...SOFT} cx="14" cy="7.5" r="2.75" />
    <circle cx="14" cy="7.5" r="2.75" />
    <circle {...SOFT} cx="10" cy="16.5" r="2.75" />
    <circle cx="10" cy="16.5" r="2.75" />
  </>,
);

/** About: the logo's five bars in a circle. */
export const AboutGlyph = glyph(
  "about",
  <>
    <circle {...SOFT} cx="12" cy="12" r="8.75" />
    <circle cx="12" cy="12" r="8.75" />
    <path d="M8 11v2M10 9.75v4.5M12 8.25v7.5M14 9.75v4.5M16 11v2" />
  </>,
);

/** Debug: a bug, drawn round. */
export const DebugGlyph = glyph(
  "debug",
  <>
    <rect {...SOFT} x="7" y="8" width="10" height="12.5" rx="5" />
    <rect x="7" y="8" width="10" height="12.5" rx="5" />
    <path d="M9.25 8.25a2.75 2.75 0 0 1 5.5 0M12 12.5v8M7 13H3.75M20.25 13H17M7.4 17.25l-2.9 1.5M16.6 17.25l2.9 1.5M7.6 9.75 5 7.75M16.4 9.75 19 7.75" />
  </>,
);

/** Dictation settings: a cursor in text. */
export const TypingGlyph = glyph(
  "typing",
  <>
    <rect {...SOFT} x="2.75" y="6" width="18.5" height="12" rx="3" />
    <rect x="2.75" y="6" width="18.5" height="12" rx="3" />
    <path d="M6.5 12h5.5M15.5 9.25v5.5" />
  </>,
);
