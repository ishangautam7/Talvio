//! `create_calendar_event`: open Google Calendar's new-event form, already
//! filled in.
//!
//! Safe by construction: the template URL only prefills the form, and the
//! event exists once the user clicks Save, so a wrong time costs a glance, not
//! a stray invitation. That is also why guests who cannot be resolved to an
//! address are left out and mentioned rather than asked about: the form is
//! right there to add them.
//!
//! Times are what a model writes for "tomorrow at nine": a naive local
//! date-time, read in the user's own time zone, or RFC 3339 with an explicit
//! offset. A local time a DST change makes ambiguous takes its earlier
//! reading, and one the clocks skip ("02:30" on a spring-forward night) is
//! moved forward by the gap, to 03:30, as calendars and the OS treat it,
//! rather than asking the user when they already said. Google's `dates=`
//! wants UTC (`20261023T031500Z`), so a naive time is converted here rather
//! than trusting the calendar's own zone to match the computer's.

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use log::{info, warn};
use reqwest::Url;
use serde_json::{json, Value};

use crate::command_mode::contacts::{looks_like_email, ContactMatch};
use crate::command_mode::registry::{
    arg_bool, arg_str, arg_strings, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS,
};

/// Length of an event with a start and no end, as Google Calendar's own
/// quick-add uses.
const DEFAULT_DURATION_MINUTES: i64 = 30;
const MIN_DURATION_MINUTES: i64 = 5;
const MAX_DURATION_MINUTES: i64 = 1440;

pub static SPEC: ToolSpec = ToolSpec {
    name: "create_calendar_event",
    description: "Create a Google Calendar event: opens the new-event form filled in with the \
                  title, time, place, notes and guests, for the user to save. Use for meetings, \
                  appointments and anything to put on the calendar (not for reminders).",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &[
        "calendar",
        "event",
        "meeting",
        "schedule",
        "appointment",
        "invite",
        "book",
    ],
    handler: run,
    on_confirm: None,
};

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "minLength": 1,
                "maxLength": 300,
                "description": "The event's title."
            },
            "start": {
                "type": "string",
                "minLength": 1,
                "maxLength": 40,
                "description": "When it starts, as local time \"YYYY-MM-DD HH:MM\" (24-hour), \
                                or just \"YYYY-MM-DD\" for an all-day event. Work it out from \
                                the local time in the context."
            },
            "end": {
                "type": "string",
                "maxLength": 40,
                "description": "When it ends, same format. Leave out to use duration_minutes."
            },
            "duration_minutes": {
                "type": "integer",
                "minimum": MIN_DURATION_MINUTES,
                "maximum": MAX_DURATION_MINUTES,
                "description": "Length in minutes when no end is given. Defaults to 30."
            },
            "all_day": {
                "type": "boolean",
                "description": "True for an all-day event."
            },
            "details": {
                "type": "string",
                "maxLength": 4000,
                "description": "Notes or agenda for the event description."
            },
            "location": {
                "type": "string",
                "maxLength": 300,
                "description": "Where it takes place, or a meeting link."
            },
            "guests": {
                "type": "array",
                "maxItems": 20,
                "items": { "type": "string", "minLength": 1, "maxLength": 320 },
                "description": "People to invite, as names or email addresses."
            }
        },
        "required": ["title", "start"],
        "additionalProperties": false
    })
}

fn run(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    run_in(ctx, args, &Local)
}

/// [`run`] in a given time zone, so tests do not depend on the machine's.
fn run_in<Tz: TimeZone>(ctx: &ToolCtx, args: Value, tz: &Tz) -> ToolOutcome {
    let Some(title) = arg_str(&args, "title") else {
        return ToolOutcome::ask("What should the event be called?", Vec::new());
    };
    let dates = match event_dates(&args, title, tz) {
        Ok(dates) => dates,
        Err(question) => return question,
    };

    let (guests, missing) = resolve_guests(ctx, &arg_strings(&args, "guests"));
    let url = match build_url(
        title,
        &dates,
        arg_str(&args, "details").unwrap_or(""),
        arg_str(&args, "location").unwrap_or(""),
        &guests,
        ctx.settings.command_google_account.trim(),
    ) {
        Ok(url) => url,
        Err(e) => return ToolOutcome::failed(e, false),
    };

    if let Err(e) = ctx.effects.open_url(url.as_str()) {
        warn!("command mode: create_calendar_event could not open the form: {e}");
        return ToolOutcome::failed(e, false);
    }
    info!(
        "command mode: create_calendar_event opened the form ({} guest(s), {} left out)",
        guests.len(),
        missing.len()
    );

    if missing.is_empty() {
        ToolOutcome::done(
            "calendarDraft",
            "Calendar event “{{title}}” is ready to save.",
            &[("title", title)],
        )
    } else {
        let left_out = missing.join(", ");
        ToolOutcome::done(
            "calendarDraftMissingGuests",
            "Calendar event “{{title}}” is ready to save. I left out {{guests}}: I don't \
             have an email address for them, so add them in the form.",
            &[("title", title), ("guests", &left_out)],
        )
    }
}

/// A time as the model wrote it: a whole day, or an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum When {
    Date(NaiveDate),
    Instant(DateTime<Utc>),
}

/// Parse a start or end time. RFC 3339 (or the same without seconds, with an
/// offset or a `Z`) keeps its offset; a naive date-time is read in `tz` by
/// [`local_instant`]; a bare date is a day.
fn parse_when<Tz: TimeZone>(raw: &str, tz: &Tz) -> Option<When> {
    let raw = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(When::Instant(dt.with_timezone(&Utc)));
    }
    // "…T03:15Z" has no seconds, so RFC 3339 refuses it; spelled as an
    // offset, the formats below read it.
    let zulu = raw
        .strip_suffix(['Z', 'z'])
        .map(|rest| format!("{rest}+00:00"));
    let with_offset = zulu.as_deref().unwrap_or(raw);
    for format in ["%Y-%m-%dT%H:%M%:z", "%Y-%m-%d %H:%M%:z"] {
        if let Ok(dt) = DateTime::parse_from_str(with_offset, format) {
            return Some(When::Instant(dt.with_timezone(&Utc)));
        }
    }
    let normalized = raw.replacen('T', " ", 1);
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&normalized, format) {
            return local_instant(tz, &naive).map(When::Instant);
        }
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .map(When::Date)
}

/// How far back [`local_instant`] looks for the wall clock in force before a
/// skipped time, in quarter hours (every real DST gap starts and ends on one):
/// a day, far more than any gap.
const GAP_SEARCH_STEPS: i64 = 24 * 4;

/// The instant a local wall-clock time names in `tz`. Ambiguous (the hour a
/// fall-back repeats): the earlier reading. Skipped (the hour a spring-forward
/// jumps over): read with the offset in force just before the jump, which
/// moves it forward by the gap, so 02:30 on a night the clocks go from 02:00
/// to 03:00 is 03:30. `None` only when no offset can be found at all.
fn local_instant<Tz: TimeZone>(tz: &Tz, naive: &NaiveDateTime) -> Option<DateTime<Utc>> {
    match tz.from_local_datetime(naive) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => Some(t.with_timezone(&Utc)),
        LocalResult::None => {
            let before = (1..=GAP_SEARCH_STEPS).find_map(|step| {
                let probe = naive.checked_sub_signed(chrono::Duration::minutes(15 * step))?;
                tz.from_local_datetime(&probe).earliest()
            })?;
            let offset = before.offset().fix();
            let utc = naive.checked_sub_signed(chrono::Duration::seconds(i64::from(
                offset.local_minus_utc(),
            )))?;
            Some(utc.and_utc())
        }
    }
}

/// Google's `dates=` value for the event, or the question to ask.
fn event_dates<Tz: TimeZone>(args: &Value, title: &str, tz: &Tz) -> Result<String, ToolOutcome> {
    let ask_start = || {
        ToolOutcome::ask(
            format!("When should “{title}” start? Say a date and a time."),
            Vec::new(),
        )
    };
    let ask_end = || {
        ToolOutcome::ask(
            format!("When should “{title}” end?"),
            Vec::new(),
        )
    };
    let start = arg_str(args, "start")
        .and_then(|raw| parse_when(raw, tz))
        .ok_or_else(ask_start)?;
    let end = match arg_str(args, "end") {
        Some(raw) => Some(parse_when(raw, tz).ok_or_else(ask_end)?),
        None => None,
    };
    let all_day = arg_bool(args, "all_day").unwrap_or(false) || matches!(start, When::Date(_));

    if all_day {
        let day_of = |when: When| match when {
            When::Date(date) => date,
            When::Instant(instant) => instant.with_timezone(tz).date_naive(),
        };
        let first = day_of(start);
        // Google's all-day end date is exclusive: a one-day event on the 23rd
        // is 20261023/20261024, and "until the 25th" means through the 25th.
        let last = end.map(day_of).unwrap_or(first);
        if last < first {
            return Err(ToolOutcome::ask(
                format!("“{title}” would end before it starts. Which day should it end?"),
                Vec::new(),
            ));
        }
        let after = last.succ_opt().ok_or_else(ask_end)?;
        return Ok(format!("{}/{}", first.format("%Y%m%d"), after.format("%Y%m%d")));
    }

    let When::Instant(start) = start else {
        return Err(ask_start());
    };
    let end = match end {
        Some(When::Instant(end)) => end,
        // A bare date as the end of a timed event says nothing about the hour.
        Some(When::Date(_)) => return Err(ask_end()),
        None => {
            let minutes = whole_number(args, "duration_minutes")
                .unwrap_or(DEFAULT_DURATION_MINUTES)
                .clamp(MIN_DURATION_MINUTES, MAX_DURATION_MINUTES);
            start + chrono::Duration::minutes(minutes)
        }
    };
    if end <= start {
        return Err(ToolOutcome::ask(
            format!("“{title}” would end before it starts. When should it end?"),
            Vec::new(),
        ));
    }
    Ok(format!(
        "{}/{}",
        start.format("%Y%m%dT%H%M%SZ"),
        end.format("%Y%m%dT%H%M%SZ")
    ))
}

/// An integer argument, accepting a whole-number float (`90.0`) the way the
/// schema validator does; `arg_i64` alone reads that as absent, and the event
/// silently got the default length.
fn whole_number(args: &Value, key: &str) -> Option<i64> {
    let value = args.get(key)?;
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.is_finite() && f.fract() == 0.0)
            .map(|f| f as i64)
    })
}

/// Guests resolved to addresses, and the names that could not be. A form the
/// user still saves is no place for a question, so the unresolved are listed
/// instead.
fn resolve_guests(ctx: &ToolCtx, queries: &[String]) -> (Vec<String>, Vec<String>) {
    let mut found: Vec<String> = Vec::new();
    let mut missing = Vec::new();
    for query in queries {
        let email = named_address(query).or_else(|| match ctx.contacts.resolve(query) {
            ContactMatch::Unique(contact) => contact.primary_email().map(str::to_string),
            ContactMatch::Ambiguous(_) | ContactMatch::None => None,
        });
        match email {
            Some(email) if !found.iter().any(|f| f.eq_ignore_ascii_case(&email)) => {
                found.push(email)
            }
            Some(_) => {}
            None => missing.push(query.clone()),
        }
    }
    if !missing.is_empty() {
        if let Some(e) = &ctx.contacts.load_error {
            warn!("command mode: contacts unavailable: {e}");
        }
    }
    (found, missing)
}

/// The address in "Name <address>", the way a contact choice reads.
fn named_address(query: &str) -> Option<String> {
    let open = query.rfind('<')?;
    let close = query[open..].find('>')? + open;
    let email = query[open + 1..close].trim();
    looks_like_email(email).then(|| email.to_string())
}

fn build_url(
    title: &str,
    dates: &str,
    details: &str,
    location: &str,
    guests: &[String],
    account: &str,
) -> Result<Url, String> {
    let mut url = Url::parse("https://calendar.google.com/calendar/render")
        .map_err(|e| format!("Could not build the Calendar address: {e}"))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("action", "TEMPLATE")
            .append_pair("text", title)
            .append_pair("dates", dates);
        if !details.is_empty() {
            query.append_pair("details", details);
        }
        if !location.is_empty() {
            query.append_pair("location", location);
        }
        if !guests.is_empty() {
            query.append_pair("add", &guests.join(","));
        }
        if !account.is_empty() {
            query.append_pair("authuser", account);
        }
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::{Contact, ContactBook};
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::registry::Platform;
    use chrono::FixedOffset;
    use std::sync::Arc;

    fn kathmandu() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600 + 45 * 60).unwrap()
    }

    fn contact(name: &str, emails: &[&str]) -> Contact {
        Contact {
            name: name.into(),
            aliases: Vec::new(),
            emails: emails.iter().map(|e| e.to_string()).collect(),
            phones: Vec::new(),
        }
    }

    fn book() -> ContactBook {
        ContactBook::new(vec![
            contact("Ron Weasley", &["ron@example.com"]),
            contact("Ginny Weasley", &["ginny@example.com"]),
            contact("Hermione Granger", &["hermione@example.com"]),
            contact("Neville Longbottom", &[]),
        ])
    }

    fn ctx(fake: &Arc<FakeEffects>, contacts: ContactBook) -> ToolCtx {
        let mut settings = crate::settings::get_default_settings();
        settings.command_google_account = String::new();
        settings.command_dry_run = false;
        let now = kathmandu().with_ymd_and_hms(2026, 10, 22, 9, 15, 0).unwrap();
        ToolCtx {
            settings,
            context: CommandContext::at(
                now,
                Some("Asia/Kathmandu".into()),
                Platform::Windows,
                None,
                None,
            ),
            effects: fake.clone(),
            contacts: Arc::new(contacts),
            platform: Platform::Windows,
            dry_run: false,
        }
    }

    fn opened_url(fake: &FakeEffects) -> String {
        let actions = fake.actions();
        assert_eq!(actions.len(), 1, "{actions:?}");
        actions[0]
            .strip_prefix("open_url ")
            .expect("an open_url action")
            .to_string()
    }

    fn pairs(url: &str) -> Vec<(String, String)> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn pair(url: &str, key: &str) -> Option<String> {
        pairs(url).into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn dates(args: Value) -> Result<String, ToolOutcome> {
        event_dates(&args, "Standup", &kathmandu())
    }

    /// Nine in the morning in Kathmandu is 03:15 UTC: the odd 45-minute
    /// offset is exactly where an hour-only conversion would go wrong.
    #[test]
    fn a_local_time_is_converted_to_utc_across_a_quarter_hour_offset() {
        assert_eq!(
            parse_when("2026-10-23 09:00", &kathmandu()),
            Some(When::Instant(Utc.with_ymd_and_hms(2026, 10, 23, 3, 15, 0).unwrap()))
        );
        // Shortly after midnight local is still the previous day in UTC.
        assert_eq!(
            dates(json!({"start": "2026-10-23 00:30"})).unwrap(),
            "20261022T184500Z/20261022T191500Z"
        );
    }

    #[test]
    fn the_t_separator_seconds_and_explicit_offsets_are_understood() {
        let nine = When::Instant(Utc.with_ymd_and_hms(2026, 10, 23, 3, 15, 0).unwrap());
        assert_eq!(parse_when("2026-10-23T09:00", &kathmandu()), Some(nine));
        assert_eq!(parse_when("2026-10-23 09:00:00", &kathmandu()), Some(nine));
        assert_eq!(parse_when("2026-10-23T09:00:00+05:45", &kathmandu()), Some(nine));
        assert_eq!(parse_when("2026-10-23T09:00+05:45", &kathmandu()), Some(nine));
        assert_eq!(parse_when("2026-10-23T03:15:00Z", &kathmandu()), Some(nine));
        assert_eq!(parse_when("2026-10-23T03:15Z", &kathmandu()), Some(nine));
        assert_eq!(parse_when("2026-10-23 03:15z", &kathmandu()), Some(nine));
        assert_eq!(
            parse_when("2026-10-23", &kathmandu()),
            Some(When::Date(NaiveDate::from_ymd_opt(2026, 10, 23).unwrap()))
        );
        assert_eq!(parse_when("tomorrow at nine", &kathmandu()), None);
        assert_eq!(parse_when("2026-13-40 09:00", &kathmandu()), None);
    }

    /// New York's 2026 rules, enough of them to have both DST changes: EST
    /// (-05:00) until 07:00 UTC on 8 March, EDT (-04:00) until 06:00 UTC on 1
    /// November. chrono has no zone database in this crate, so the zone is
    /// written out.
    #[derive(Debug, Clone, Copy)]
    struct NewYork2026;

    impl NewYork2026 {
        fn est() -> FixedOffset {
            FixedOffset::west_opt(5 * 3600).unwrap()
        }
        fn edt() -> FixedOffset {
            FixedOffset::west_opt(4 * 3600).unwrap()
        }
    }

    impl TimeZone for NewYork2026 {
        type Offset = FixedOffset;

        fn from_offset(_: &FixedOffset) -> Self {
            NewYork2026
        }
        fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<FixedOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).unwrap())
        }
        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<FixedOffset> {
            // Every offset whose reading of `local` lands where that offset
            // is in force.
            let fits: Vec<FixedOffset> = [Self::edt(), Self::est()]
                .into_iter()
                .filter(|offset| {
                    let utc = *local - chrono::Duration::seconds(i64::from(offset.local_minus_utc()));
                    self.offset_from_utc_datetime(&utc) == *offset
                })
                .collect();
            match fits.as_slice() {
                [] => LocalResult::None,
                [one] => LocalResult::Single(*one),
                // EDT first: its reading is the earlier instant.
                [first, second, ..] => LocalResult::Ambiguous(*first, *second),
            }
        }
        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }
        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            let spring = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap().and_hms_opt(7, 0, 0).unwrap();
            let fall = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap().and_hms_opt(6, 0, 0).unwrap();
            if *utc >= spring && *utc < fall {
                Self::edt()
            } else {
                Self::est()
            }
        }
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Option<When> {
        Some(When::Instant(Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()))
    }

    /// A time the clocks skip is moved forward by the gap rather than asked
    /// about: 02:30 on the spring-forward night is 03:30 EDT. Its neighbours,
    /// and the fall-back hour (the earlier reading), are read as written.
    #[test]
    fn a_skipped_local_time_moves_forward_by_the_gap() {
        let ny = NewYork2026;
        assert_eq!(parse_when("2026-03-08 02:30", &ny), utc(2026, 3, 8, 7, 30));
        assert_eq!(parse_when("2026-03-08 03:30", &ny), utc(2026, 3, 8, 7, 30));
        assert_eq!(parse_when("2026-03-08 02:00", &ny), utc(2026, 3, 8, 7, 0));
        assert_eq!(parse_when("2026-03-08 01:30", &ny), utc(2026, 3, 8, 6, 30));
        assert_eq!(parse_when("2026-11-01 01:30", &ny), utc(2026, 11, 1, 5, 30));
        assert_eq!(parse_when("2026-10-23 09:00", &ny), utc(2026, 10, 23, 13, 0));
        assert_eq!(
            event_dates(&json!({"start": "2026-03-08 02:30"}), "Standup", &ny).unwrap(),
            "20260308T073000Z/20260308T080000Z"
        );
    }

    #[test]
    fn the_end_defaults_to_thirty_minutes_or_the_given_duration_or_the_given_end() {
        assert_eq!(
            dates(json!({"start": "2026-10-23 09:00"})).unwrap(),
            "20261023T031500Z/20261023T034500Z"
        );
        assert_eq!(
            dates(json!({"start": "2026-10-23 09:00", "duration_minutes": 90})).unwrap(),
            "20261023T031500Z/20261023T044500Z"
        );
        // A whole-number float passes the schema, so it must count too.
        assert_eq!(
            dates(json!({"start": "2026-10-23 09:00", "duration_minutes": 90.0})).unwrap(),
            "20261023T031500Z/20261023T044500Z"
        );
        assert_eq!(
            dates(json!({"start": "2026-10-23 09:00", "end": "2026-10-23 11:30"})).unwrap(),
            "20261023T031500Z/20261023T054500Z"
        );
    }

    /// Google's all-day end date is exclusive, so a single day ends on the
    /// next one and an inclusive end date gains a day.
    #[test]
    fn an_all_day_event_uses_dates_with_an_exclusive_end() {
        assert_eq!(
            dates(json!({"start": "2026-10-23"})).unwrap(),
            "20261023/20261024"
        );
        assert_eq!(
            dates(json!({"start": "2026-10-23 09:00", "all_day": true})).unwrap(),
            "20261023/20261024"
        );
        assert_eq!(
            dates(json!({"start": "2026-10-23", "end": "2026-10-25", "all_day": true})).unwrap(),
            "20261023/20261026"
        );
        // The local day decides, not the UTC one: 00:30 in Kathmandu is the
        // 22nd in UTC but the 23rd here.
        assert_eq!(
            dates(json!({"start": "2026-10-23 00:30", "all_day": true})).unwrap(),
            "20261023/20261024"
        );
        assert_eq!(
            dates(json!({"start": "2026-12-31"})).unwrap(),
            "20261231/20270101"
        );
    }

    #[test]
    fn a_time_that_cannot_be_read_is_asked_for() {
        assert_eq!(
            dates(json!({"start": "next Tuesday-ish"})),
            Err(ToolOutcome::ask(
                "When should “Standup” start? Say a date and a time.",
                vec![]
            ))
        );
        assert_eq!(
            dates(json!({"start": "2026-10-23 09:00", "end": "soon"})),
            Err(ToolOutcome::ask("When should “Standup” end?", vec![]))
        );
        assert!(matches!(
            dates(json!({"start": "2026-10-23 09:00", "end": "2026-10-23 08:00"})),
            Err(ToolOutcome::AskUser { .. })
        ));
        assert!(matches!(
            dates(json!({"start": "2026-10-25", "end": "2026-10-23"})),
            Err(ToolOutcome::AskUser { .. })
        ));
    }

    /// The whole form decodes back to exactly what was asked for, and the
    /// handler opens it.
    #[test]
    fn the_form_url_carries_every_field_encoded() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book());
        let outcome = run_in(
            &ctx,
            json!({
                "title": "Design review: café & Q4",
                "start": "2026-10-23 09:00",
                "details": "Agenda:\n1. Numbers\n2. 50% plan",
                "location": "Room 4, Lalitpur",
                "guests": ["Ron", "hermione@example.com"]
            }),
            &kathmandu(),
        );
        assert_eq!(
            outcome,
            ToolOutcome::done(
                "calendarDraft",
                "Calendar event “{{title}}” is ready to save.",
                &[("title", "Design review: café & Q4")]
            )
        );
        let url = opened_url(&fake);
        assert!(
            url.starts_with("https://calendar.google.com/calendar/render?action=TEMPLATE&"),
            "{url}"
        );
        assert!(url.contains("%0A"), "{url}");
        assert_eq!(
            pairs(&url),
            vec![
                ("action".to_string(), "TEMPLATE".to_string()),
                ("text".to_string(), "Design review: café & Q4".to_string()),
                (
                    "dates".to_string(),
                    "20261023T031500Z/20261023T034500Z".to_string()
                ),
                (
                    "details".to_string(),
                    "Agenda:\n1. Numbers\n2. 50% plan".to_string()
                ),
                ("location".to_string(), "Room 4, Lalitpur".to_string()),
                (
                    "add".to_string(),
                    "ron@example.com,hermione@example.com".to_string()
                ),
            ]
        );
    }

    /// Guests who cannot be resolved are not asked about; they are left out
    /// and named, because the user is about to see the form anyway.
    #[test]
    fn unresolved_guests_are_left_out_and_named() {
        let fake = Arc::new(FakeEffects::default());
        let ctx = ctx(&fake, book());
        let outcome = run_in(
            &ctx,
            json!({
                "title": "Lunch",
                "start": "2026-10-23 12:00",
                "guests": ["Weasley", "Neville", "Dumbledore", "Ginny Weasley <ginny@example.com>"]
            }),
            &kathmandu(),
        );
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done");
        };
        assert_eq!(feedback.key, "commandMode.feedback.calendarDraftMissingGuests");
        assert!(
            feedback.render_en().contains("Weasley, Neville, Dumbledore"),
            "{}",
            feedback.render_en()
        );
        assert_eq!(
            pair(&opened_url(&fake), "add").as_deref(),
            Some("ginny@example.com")
        );
    }

    #[test]
    fn the_google_account_setting_becomes_authuser() {
        let fake = Arc::new(FakeEffects::default());
        let mut tool_ctx = ctx(&fake, book());
        tool_ctx.settings.command_google_account = "1".into();
        run_in(
            &tool_ctx,
            json!({"title": "Lunch", "start": "2026-10-23T12:00:00+05:45"}),
            &kathmandu(),
        );
        let url = opened_url(&fake);
        assert_eq!(pair(&url, "authuser").as_deref(), Some("1"));
        assert_eq!(pair(&url, "add"), None);
        assert_eq!(pair(&url, "details"), None);
    }

    /// The real handler reads naive times in the machine's zone; with an
    /// explicit offset the result does not depend on where the test runs.
    #[test]
    fn the_handler_opens_the_form_in_the_local_zone() {
        let fake = Arc::new(FakeEffects::default());
        let tool_ctx = ctx(&fake, book());
        let outcome = run(
            &tool_ctx,
            json!({"title": "Call", "start": "2026-10-23T09:00:00+05:45", "duration_minutes": 60}),
        );
        assert!(matches!(outcome, ToolOutcome::Done { .. }), "{outcome:?}");
        assert_eq!(
            pair(&opened_url(&fake), "dates").as_deref(),
            Some("20261023T031500Z/20261023T041500Z")
        );
    }

    #[test]
    fn a_bad_time_opens_nothing() {
        let fake = Arc::new(FakeEffects::default());
        let tool_ctx = ctx(&fake, book());
        let outcome = run_in(
            &tool_ctx,
            json!({"title": "Call", "start": "later"}),
            &kathmandu(),
        );
        assert!(matches!(outcome, ToolOutcome::AskUser { .. }), "{outcome:?}");
        assert!(fake.actions().is_empty());
    }

    #[test]
    fn a_browser_that_will_not_open_fails_without_escalating() {
        let fake = Arc::new(FakeEffects {
            fail_with: Some("No browser".into()),
            ..Default::default()
        });
        let tool_ctx = ctx(&fake, book());
        assert_eq!(
            run_in(
                &tool_ctx,
                json!({"title": "Call", "start": "2026-10-23 09:00"}),
                &kathmandu()
            ),
            ToolOutcome::failed("No browser", false)
        );
    }

    #[test]
    fn the_schema_requires_a_title_and_a_start_and_bounds_the_duration() {
        use crate::command_mode::schema::{check_schema, validate};
        let schema = schema();
        assert!(check_schema(&schema).is_ok());
        assert!(validate(&schema, &json!({"title": "x", "start": "2026-10-23"})).is_ok());
        assert!(validate(&schema, &json!({"title": "x"})).is_err());
        assert!(validate(
            &schema,
            &json!({"title": "x", "start": "2026-10-23 09:00", "duration_minutes": 2})
        )
        .is_err());
        assert!(validate(
            &schema,
            &json!({"title": "x", "start": "2026-10-23 09:00", "duration_minutes": 2000})
        )
        .is_err());
    }
}
