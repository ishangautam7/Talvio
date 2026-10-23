//! `play_media`: "play Believer by Imagine Dragons", "play some lo-fi".
//!
//! YouTube (the default) and YouTube Music start playing: the tool fetches
//! YouTube's own results page, takes the first real result out of the data
//! the page is built from (the first `videoRenderer`, so an ad or a Short
//! listed above it is skipped), and opens its watch page, which autoplays.
//! No API key is involved, so this depends on the page's format; when the
//! lookup fails (offline, a consent page, a format change) the results page
//! itself is opened and the feedback says so, so the user is one click away
//! rather than nowhere.
//!
//! Spotify is only used when the user names it. Without its Web API (an
//! OAuth login this app does not have) Spotify cannot be told to start the
//! top result of a search, so the tool opens the search in the Spotify app
//! (or the web player when the app is not installed) and says that the user
//! picks what to play. That is why "play" with no service goes to YouTube.
//!
//! The fetch is a read ([`Effects::fetch_text`]), so a dry run looks the
//! result up for real and only records the page it would have opened.
//!
//! [`Effects::fetch_text`]: crate::command_mode::effects::Effects::fetch_text

use log::{info, warn};
use reqwest::Url;
use serde_json::{json, Value};
use std::time::Duration;

use super::web_search::encode_component;
use crate::command_mode::registry::{arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS};
use crate::command_mode::sites;

/// Long enough for a slow connection, short of the executor's 60 s limit.
const FETCH_TIMEOUT: Duration = Duration::from_secs(12);
/// Accepts YouTube's cookie notice up front. Without it a visitor from the EU
/// is redirected to consent.youtube.com, which lists no results.
const CONSENT_COOKIE: (&str, &str) = ("Cookie", "SOCS=CAI");
/// YouTube's results filters (`sp`): videos only, and playlists only.
const VIDEOS_ONLY: &str = "EgIQAQ==";
const PLAYLISTS_ONLY: &str = "EgIQAw==";
/// How far after a result's id its title may sit in the page.
const TITLE_WINDOW: usize = 20_000;

pub static SPEC: ToolSpec = ToolSpec {
    name: "play_media",
    description: "Find a song, video, artist or playlist by name and start playing it, on \
                  YouTube unless the user names another service (\"play Believer\", \"play some \
                  lo-fi\", \"play Taylor Swift on Spotify\"). Use media_control instead to \
                  pause, resume or skip what is already playing.",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &[
        "play", "song", "music", "video", "listen", "watch", "youtube", "spotify", "playlist",
        "album", "artist", "track",
    ],
    handler: run,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "maxLength": 300,
                "description": "What to play, as the user would type it into a search box: \
                                \"Believer Imagine Dragons\", \"lofi hip hop\", \"MrBeast latest \
                                video\". Leave the service's name out."
            },
            "service": {
                "type": "string",
                "enum": ["auto", "youtube", "youtube_music", "spotify"],
                "description": "Where to play. auto (the default) is YouTube; set another only \
                                when the user names it."
            },
            "kind": {
                "type": "string",
                "enum": ["any", "song", "video", "playlist"],
                "description": "playlist when the user asks for a playlist or a mix; otherwise \
                                any (the default)."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

/// Which YouTube front end to open the result in. Both share video and
/// playlist ids, so one lookup serves both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    YouTube,
    Music,
}

impl Front {
    fn label(self) -> &'static str {
        match self {
            Front::YouTube => "YouTube",
            Front::Music => "YouTube Music",
        }
    }

    fn host(self) -> &'static str {
        match self {
            Front::YouTube => "https://www.youtube.com",
            Front::Music => "https://music.youtube.com",
        }
    }

    /// The page that plays `hit`.
    fn play_url(self, hit: &Hit) -> String {
        let host = self.host();
        match (&hit.video, &hit.playlist) {
            (Some(video), Some(list)) => format!("{host}/watch?v={video}&list={list}"),
            (Some(video), None) => format!("{host}/watch?v={video}"),
            (None, Some(list)) => format!("{host}/playlist?list={list}"),
            (None, None) => self.results_url(""),
        }
    }

    /// The results page a person would see, for the fallback.
    fn results_url(self, query: &str) -> String {
        let query = encode_component(query);
        match self {
            Front::YouTube => format!("https://www.youtube.com/results?search_query={query}"),
            Front::Music => format!("https://music.youtube.com/search?q={query}"),
        }
    }
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(query) = arg_str(&args, "query") else {
        return ToolOutcome::ask("What should I play?", Vec::new());
    };
    let playlist = arg_str(&args, "kind") == Some("playlist");
    match arg_str(&args, "service").unwrap_or("auto") {
        "spotify" => spotify(ctx, query),
        "youtube_music" => youtube(ctx, query, playlist, Front::Music),
        _ => youtube(ctx, query, playlist, Front::YouTube),
    }
}

fn youtube(ctx: &ToolCtx, query: &str, playlist: bool, front: Front) -> ToolOutcome {
    let lookup = lookup_url(query, playlist);
    let hit = match ctx
        .effects
        .fetch_text(lookup.as_str(), &[CONSENT_COOKIE], FETCH_TIMEOUT)
    {
        Ok(page) => {
            let hit = if playlist {
                first_playlist(&page)
            } else {
                first_video(&page)
            };
            if hit.is_none() {
                warn!(
                    "command mode: play_media found no result in YouTube's page ({} bytes)",
                    page.len()
                );
            }
            hit
        }
        Err(e) => {
            warn!("command mode: play_media could not look the result up: {e}");
            None
        }
    };

    let Some(hit) = hit else {
        return open(
            ctx,
            &front.results_url(query),
            ToolOutcome::done(
                "mediaResultsOpened",
                "Opened {{service}}'s results for “{{query}}”.",
                &[("service", front.label()), ("query", query)],
            ),
        );
    };
    let title = hit.title.clone().unwrap_or_else(|| query.to_string());
    info!(
        "command mode: play_media is opening a result on {}",
        front.label()
    );
    open(
        ctx,
        &front.play_url(&hit),
        ToolOutcome::done(
            "mediaPlaying",
            "Playing “{{title}}” on {{service}}.",
            &[("title", &title), ("service", front.label())],
        ),
    )
}

/// Spotify cannot start a search's top result without its Web API, so this
/// opens the search and says so.
fn spotify(ctx: &ToolCtx, query: &str) -> ToolOutcome {
    let installed = sites::named("spotify").is_some_and(|site| {
        ctx.effects
            .installed_apps()
            .iter()
            .any(|app| sites::is_own_app(site, &app.name))
    });
    let encoded = encode_component(query);
    let url = if installed {
        format!("spotify:search:{encoded}")
    } else {
        format!("https://open.spotify.com/search/{encoded}")
    };
    open(
        ctx,
        &url,
        ToolOutcome::done(
            "spotifyResultsOpened",
            "Opened Spotify's results for “{{query}}”. Spotify can't start a search by \
             itself, so pick what to play.",
            &[("query", query)],
        ),
    )
}

/// Open `url` and report `success`, or the failure to open it.
fn open(ctx: &ToolCtx, url: &str, success: ToolOutcome) -> ToolOutcome {
    match ctx.effects.open_url(url) {
        Ok(()) => success,
        Err(e) => {
            warn!("command mode: play_media could not open the page: {e}");
            ToolOutcome::failed(e, false)
        }
    }
}

/// The results page fetched for the lookup: videos only (no channels or
/// Shorts shelves to skip over), or playlists only.
fn lookup_url(query: &str, playlist: bool) -> Url {
    let filter = if playlist {
        PLAYLISTS_ONLY
    } else {
        VIDEOS_ONLY
    };
    Url::parse_with_params(
        "https://www.youtube.com/results",
        &[("search_query", query), ("sp", filter)],
    )
    .expect("a fixed https address with encoded parameters parses")
}

/// The result to play.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hit {
    video: Option<String>,
    playlist: Option<String>,
    title: Option<String>,
}

/// The first video result. Prefers the first `videoRenderer` (an ordinary
/// result); falls back to the first video id anywhere in the page.
fn first_video(page: &str) -> Option<Hit> {
    const RENDERER: &str = r#""videoRenderer":{"videoId":""#;
    if let Some(at) = page.find(RENDERER) {
        let start = at + RENDERER.len();
        if let Some(video) = id_at(page, start, is_video_id) {
            let title = string_after(page, start, r#""title":{"runs":[{"text":"#);
            return Some(Hit {
                video: Some(video),
                playlist: None,
                title,
            });
        }
    }
    find_id(page, r#""videoId":""#, is_video_id).map(|video| Hit {
        video: Some(video),
        playlist: None,
        title: None,
    })
}

/// The first playlist result, with the video it starts on when the page says.
fn first_playlist(page: &str) -> Option<Hit> {
    // Playlist results are `lockupViewModel`s whose `contentId` is the list.
    let mut from = 0;
    let (list, at) = loop {
        let found = page[from..].find(r#""contentId":""#)? + from;
        let start = found + r#""contentId":""#.len();
        match id_at(page, start, is_playlist_id) {
            Some(list) => break (list, start),
            None => from = start,
        }
    };
    let video = first_video_of(page, &list);
    let title = string_after(page, at, r#""title":{"content":"#);
    Some(Hit {
        video,
        playlist: Some(list),
        title,
    })
}

/// The video playlist `list` starts on: its `watchEndpoint` names both.
fn first_video_of(page: &str, list: &str) -> Option<String> {
    const MARKER: &str = r#""watchEndpoint":{"videoId":""#;
    let follows = format!(r#"","playlistId":"{list}""#);
    let mut from = 0;
    while let Some(found) = page[from..].find(MARKER) {
        let start = from + found + MARKER.len();
        if let Some(video) = id_at(page, start, is_video_id) {
            if page[start + video.len()..].starts_with(&follows) {
                return Some(video);
            }
        }
        from = start;
    }
    None
}

/// The id that starts at byte `start` and runs to the next `"`, if `valid`.
fn id_at(page: &str, start: usize, valid: fn(&str) -> bool) -> Option<String> {
    let rest = page.get(start..)?;
    let id = &rest[..rest.find('"')?];
    valid(id).then(|| id.to_string())
}

/// The first id after any occurrence of `marker` that is `valid`.
fn find_id(page: &str, marker: &str, valid: fn(&str) -> bool) -> Option<String> {
    let mut from = 0;
    while let Some(found) = page[from..].find(marker) {
        let start = from + found + marker.len();
        if let Some(id) = id_at(page, start, valid) {
            return Some(id);
        }
        from = start;
    }
    None
}

/// The JSON string that follows the first `key` within [`TITLE_WINDOW`]
/// bytes after `start`, unescaped.
fn string_after(page: &str, start: usize, key: &str) -> Option<String> {
    let mut end = (start + TITLE_WINDOW).min(page.len());
    while !page.is_char_boundary(end) {
        end -= 1;
    }
    let window = &page[start..end];
    let quote = start + window.find(key)? + key.len();
    json_string_at(page, quote)
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
}

/// The JSON string literal whose opening quote is at byte `quote`.
fn json_string_at(page: &str, quote: usize) -> Option<String> {
    let bytes = page.as_bytes();
    if bytes.get(quote) != Some(&b'"') {
        return None;
    }
    let mut i = quote + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return serde_json::from_str(&page[quote..=i]).ok(),
            _ => i += 1,
        }
    }
    None
}

/// YouTube video ids are 11 characters of URL-safe base64.
fn is_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(is_id_byte)
}

/// Playlist ids: a two-letter kind (PL, OL, RD, UU…) and at least ten more
/// URL-safe characters. A video id in the same field is not one.
fn is_playlist_id(id: &str) -> bool {
    id.len() >= 12
        && id.bytes().all(is_id_byte)
        && ["PL", "OL", "RD", "UU", "FL", "LL"]
            .iter()
            .any(|prefix| id.starts_with(prefix))
}

fn is_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::ContactBook;
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::effects::{AppEntry, DryRunEffects, Effects, RecordsActions};
    use crate::command_mode::registry::{validate_args, Platform};
    use chrono::{TimeZone, Utc};
    use std::sync::Arc;

    /// Trimmed from a real results page for "believer imagine dragons": an ad
    /// slot with a video id comes first, then the first ordinary result.
    const RESULTS_PAGE: &str = r#"<html><script>var ytInitialData = {"responseContext":{},"estimatedResults":"10790077","contents":{"twoColumnSearchResultsRenderer":{"primaryContents":{"sectionListRenderer":{"contents":[{"itemSectionRenderer":{"contents":[{"adSlotRenderer":{"enablePacfLoggingWeb":true,"fulfillmentContent":{"fulfilledLayout":{"inFeedAdLayoutRenderer":{"renderingContent":{"promotedVideoRenderer":{"videoId":"AdAdAdAdAd1"}}}}}}},{"videoRenderer":{"videoId":"7wtfhZwyrcc","thumbnail":{"thumbnails":[{"url":"https://i.ytimg.com/vi/7wtfhZwyrcc/hq720.jpg?sqp=-oaymwEc\u0026rs=AOn4CLAq","width":360,"height":202}]},"title":{"runs":[{"text":"Imagine Dragons - Believer (Official Music Video)"}],"accessibility":{"accessibilityData":{"label":"Imagine Dragons - Believer 3 minutes"}}},"longBylineText":{"runs":[{"text":"ImagineDragonsVEVO"}]}}},{"videoRenderer":{"videoId":"W0DM5lcj6mw","title":{"runs":[{"text":"Imagine Dragons - Believer (Lyrics)"}]}}}]}}]}}}}};</script></html>"#;

    /// Trimmed from a real playlist-filtered results page for "lofi".
    const PLAYLIST_PAGE: &str = r#"{"contents":[{"lockupViewModel":{"contentImage":{"collectionThumbnailViewModel":{}},"contentId":"PLPsZHoZ0uuorKaKyE69ShxwAj4X1Goa_U","contentType":"LOCKUP_CONTENT_TYPE_PLAYLIST","metadata":{"lockupMetadataViewModel":{"title":{"content":"Chill Drive \ud83d\ude97 Lofi Beats \"To Focus\""}}},"rendererContext":{"commandContext":{"onTap":{"innertubeCommand":{"commandMetadata":{"webCommandMetadata":{"url":"/watch?v=Qyq3olBMWI0\u0026list=PLPsZHoZ0uuorKaKyE69ShxwAj4X1Goa_U"}},"watchEndpoint":{"videoId":"Qyq3olBMWI0","playlistId":"PLPsZHoZ0uuorKaKyE69ShxwAj4X1Goa_U"}}}}}}}]}"#;

    const LOOKUP: &str = "https://www.youtube.com/results?search_query=";

    fn ctx(effects: Arc<dyn Effects>) -> ToolCtx {
        let platform = Platform::Windows;
        ToolCtx {
            settings: crate::settings::get_default_settings(),
            context: CommandContext::at(
                Utc.with_ymd_and_hms(2026, 10, 23, 9, 0, 0).unwrap(),
                None,
                platform,
                None,
                None,
            ),
            effects,
            contacts: Arc::new(ContactBook::new(Vec::new())),
            platform,
            dry_run: false,
        }
    }

    fn fake_with(page: Result<&str, &str>) -> Arc<FakeEffects> {
        Arc::new(FakeEffects {
            pages: vec![(
                LOOKUP.to_string(),
                page.map(str::to_string).map_err(str::to_string),
            )],
            ..Default::default()
        })
    }

    fn english(outcome: &ToolOutcome) -> String {
        match outcome {
            ToolOutcome::Done { feedback } => feedback.render_en().to_string(),
            other => panic!("expected done, got {other:?}"),
        }
    }

    #[test]
    fn the_first_real_result_is_found_past_an_ad() {
        assert_eq!(
            first_video(RESULTS_PAGE),
            Some(Hit {
                video: Some("7wtfhZwyrcc".into()),
                playlist: None,
                title: Some("Imagine Dragons - Believer (Official Music Video)".into()),
            })
        );
    }

    /// A page without a `videoRenderer` still yields the first valid id.
    #[test]
    fn without_a_renderer_any_video_id_will_do() {
        let page = r#"{"x":{"videoId":"short"},"y":{"videoId":"abcDEF12_-x"}}"#;
        assert_eq!(
            first_video(page).and_then(|hit| hit.video),
            Some("abcDEF12_-x".to_string())
        );
        assert_eq!(first_video("<html>consent</html>"), None);
    }

    #[test]
    fn a_playlist_is_found_with_its_first_video_and_title() {
        assert_eq!(
            first_playlist(PLAYLIST_PAGE),
            Some(Hit {
                video: Some("Qyq3olBMWI0".into()),
                playlist: Some("PLPsZHoZ0uuorKaKyE69ShxwAj4X1Goa_U".into()),
                title: Some("Chill Drive 🚗 Lofi Beats \"To Focus\"".into()),
            })
        );
        assert_eq!(first_playlist(RESULTS_PAGE), None);
    }

    #[test]
    fn play_opens_the_first_video_on_youtube() {
        let fake = fake_with(Ok(RESULTS_PAGE));
        let args = json!({"query": "Believer Imagine Dragons"});
        assert!(validate_args(&SPEC, &args).is_ok());
        let outcome = run(&ctx(fake.clone()), args);
        assert_eq!(
            english(&outcome),
            "Playing “Imagine Dragons - Believer (Official Music Video)” on YouTube."
        );
        let ToolOutcome::Done { feedback } = &outcome else {
            unreachable!()
        };
        assert_eq!(feedback.key, "commandMode.feedback.mediaPlaying");
        assert_eq!(
            fake.actions(),
            vec!["open_url https://www.youtube.com/watch?v=7wtfhZwyrcc".to_string()]
        );
        assert_eq!(
            fake.fetched(),
            vec![
                "https://www.youtube.com/results?search_query=Believer+Imagine+Dragons&sp=EgIQAQ%3D%3D"
                    .to_string()
            ]
        );
    }

    #[test]
    fn youtube_music_plays_the_same_result_there() {
        let fake = fake_with(Ok(RESULTS_PAGE));
        let outcome = run(
            &ctx(fake.clone()),
            json!({"query": "believer", "service": "youtube_music", "kind": "song"}),
        );
        assert!(english(&outcome).ends_with("on YouTube Music."));
        assert_eq!(
            fake.actions(),
            vec!["open_url https://music.youtube.com/watch?v=7wtfhZwyrcc".to_string()]
        );
    }

    #[test]
    fn a_playlist_request_plays_the_playlist() {
        let fake = fake_with(Ok(PLAYLIST_PAGE));
        let outcome = run(
            &ctx(fake.clone()),
            json!({"query": "lofi", "kind": "playlist"}),
        );
        assert!(english(&outcome).starts_with("Playing “Chill Drive"));
        assert_eq!(
            fake.actions(),
            vec![
                "open_url https://www.youtube.com/watch?v=Qyq3olBMWI0&list=PLPsZHoZ0uuorKaKyE69ShxwAj4X1Goa_U"
                    .to_string()
            ]
        );
        assert!(fake.fetched()[0].ends_with("&sp=EgIQAw%3D%3D"));
    }

    /// Offline, or a page in a format this no longer understands: the user
    /// still gets the results, and is told that is what they got.
    #[test]
    fn a_failed_lookup_opens_the_results_page_instead() {
        for fake in [
            fake_with(Err("offline")),
            fake_with(Ok("<html>consent</html>")),
        ] {
            let outcome = run(&ctx(fake.clone()), json!({"query": "some lo-fi"}));
            assert_eq!(
                english(&outcome),
                "Opened YouTube's results for “some lo-fi”."
            );
            let ToolOutcome::Done { feedback } = &outcome else {
                unreachable!()
            };
            assert_eq!(feedback.key, "commandMode.feedback.mediaResultsOpened");
            assert_eq!(
                fake.actions(),
                vec![
                    "open_url https://www.youtube.com/results?search_query=some%20lo-fi"
                        .to_string()
                ]
            );
        }
        let fake = fake_with(Err("offline"));
        run(
            &ctx(fake.clone()),
            json!({"query": "believer", "service": "youtube_music"}),
        );
        assert_eq!(
            fake.actions(),
            vec!["open_url https://music.youtube.com/search?q=believer".to_string()]
        );
    }

    #[test]
    fn spotify_opens_its_search_in_the_app_when_installed() {
        let fake = Arc::new(FakeEffects {
            apps: vec![AppEntry {
                name: "Spotify".into(),
                launch: "Spotify.lnk".into(),
            }],
            ..Default::default()
        });
        let outcome = run(
            &ctx(fake.clone()),
            json!({"query": "Taylor Swift", "service": "spotify"}),
        );
        assert_eq!(
            english(&outcome),
            "Opened Spotify's results for “Taylor Swift”. Spotify can't start a search by \
             itself, so pick what to play."
        );
        assert_eq!(
            fake.actions(),
            vec!["open_url spotify:search:Taylor%20Swift".to_string()]
        );
        assert!(fake.fetched().is_empty(), "Spotify needs no lookup");
    }

    #[test]
    fn spotify_without_the_app_opens_the_web_player() {
        let fake = Arc::new(FakeEffects::default());
        let outcome = run(
            &ctx(fake.clone()),
            json!({"query": "Taylor Swift", "service": "spotify"}),
        );
        let ToolOutcome::Done { feedback } = &outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(feedback.key, "commandMode.feedback.spotifyResultsOpened");
        assert_eq!(
            fake.actions(),
            vec!["open_url https://open.spotify.com/search/Taylor%20Swift".to_string()]
        );
    }

    /// "play" with no service is YouTube even with Spotify installed.
    #[test]
    fn auto_prefers_youtube_over_an_installed_spotify() {
        let fake = Arc::new(FakeEffects {
            apps: vec![AppEntry {
                name: "Spotify".into(),
                launch: "Spotify.lnk".into(),
            }],
            pages: vec![(LOOKUP.to_string(), Ok(RESULTS_PAGE.to_string()))],
            ..Default::default()
        });
        run(
            &ctx(fake.clone()),
            json!({"query": "believer", "service": "auto"}),
        );
        assert_eq!(
            fake.actions(),
            vec!["open_url https://www.youtube.com/watch?v=7wtfhZwyrcc".to_string()]
        );
    }

    /// A dry run still looks the result up, and records what it would open.
    #[test]
    fn a_dry_run_looks_up_but_only_records_the_page() {
        let inner = FakeEffects {
            pages: vec![(LOOKUP.to_string(), Ok(RESULTS_PAGE.to_string()))],
            fail_with: Some("would have run".into()),
            ..Default::default()
        };
        let dry = Arc::new(DryRunEffects::new(inner));
        let outcome = run(&ctx(dry.clone()), json!({"query": "believer"}));
        assert!(english(&outcome).starts_with("Playing “Imagine Dragons"));
        assert_eq!(
            dry.recorded(),
            vec!["open URL https://www.youtube.com/watch?v=7wtfhZwyrcc".to_string()]
        );
    }

    #[test]
    fn a_browser_that_will_not_open_is_a_failure() {
        let fake = Arc::new(FakeEffects {
            pages: vec![(LOOKUP.to_string(), Ok(RESULTS_PAGE.to_string()))],
            fail_with: Some("No browser".into()),
            ..Default::default()
        });
        assert_eq!(
            run(&ctx(fake), json!({"query": "believer"})),
            ToolOutcome::failed("No browser", false)
        );
    }

    #[test]
    fn the_schema_takes_a_query_and_known_choices_only() {
        assert!(validate_args(&SPEC, &json!({"query": "x"})).is_ok());
        assert!(validate_args(
            &SPEC,
            &json!({"query": "x", "service": "youtube_music", "kind": "playlist"})
        )
        .is_ok());
        assert!(validate_args(&SPEC, &json!({})).is_err());
        assert!(validate_args(&SPEC, &json!({"query": "x", "service": "tidal"})).is_err());
        assert!(validate_args(&SPEC, &json!({"query": "x", "kind": "album"})).is_err());
        assert!(validate_args(&SPEC, &json!({"query": "x", "volume": 3})).is_err());
    }

    #[test]
    fn ids_are_checked_for_shape() {
        assert!(is_video_id("7wtfhZwyrcc"));
        assert!(!is_video_id("7wtfhZwyrc"));
        assert!(!is_video_id("7wtfhZwyr\"c"));
        assert!(is_playlist_id("PLPsZHoZ0uuorKaKyE69ShxwAj4X1Goa_U"));
        assert!(!is_playlist_id("7wtfhZwyrcc"));
    }

    /// Run by hand: `cargo test --lib play_media_live -- --ignored --nocapture`.
    #[test]
    #[ignore = "reaches youtube.com"]
    fn play_media_live_youtube_lookup_finds_a_video() {
        let url = lookup_url("believer imagine dragons", false);
        let page = crate::command_mode::effects::http::fetch_text(
            url.as_str(),
            &[CONSENT_COOKIE],
            FETCH_TIMEOUT,
        )
        .expect("YouTube answered");
        let hit = first_video(&page).expect("a video in the results");
        println!("video: {hit:?} (page {} bytes)", page.len());
        assert!(is_video_id(hit.video.as_deref().unwrap()));

        let url = lookup_url("lofi hip hop", true);
        let page = crate::command_mode::effects::http::fetch_text(
            url.as_str(),
            &[CONSENT_COOKIE],
            FETCH_TIMEOUT,
        )
        .expect("YouTube answered");
        let hit = first_playlist(&page).expect("a playlist in the results");
        println!("playlist: {hit:?}");
    }
}
