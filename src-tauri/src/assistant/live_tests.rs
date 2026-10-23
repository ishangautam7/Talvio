//! `#[ignore]`d checks of the assistant's model-and-tools loop against the
//! model this machine's installed settings use. Nothing runs on the computer:
//! `do_on_computer` is answered here with a dry-run report, the way command
//! mode's Dry run answers it, and every other tool with a canned line.
//!
//! ```text
//! cargo test --lib assistant::live_tests -- --ignored --nocapture
//! ```

use std::sync::Mutex;
use std::time::Instant;

use futures_util::future::BoxFuture;

use super::act;
use super::phone::request_messages;
use super::turn::{self, TurnTools};
use crate::command_mode::llm::Usage;
use crate::command_mode::registry::Feedback;
use crate::command_mode::{CommandOrigin, CommandReport, CommandStatus};
use crate::llm_client::ToolCall;

/// Records every tool call and answers `do_on_computer` as a dry run would.
#[derive(Default)]
struct Recording {
    calls: Mutex<Vec<(String, String)>>,
}

impl TurnTools for Recording {
    fn round_started(&self, calls: &[ToolCall]) {
        for call in calls {
            println!("tool call: {} {}", call.name, call.arguments);
        }
    }

    fn run<'a>(&'a self, call: &'a ToolCall) -> BoxFuture<'a, String> {
        self.calls
            .lock()
            .unwrap()
            .push((call.name.clone(), call.arguments.clone()));
        Box::pin(async move {
            if call.name != act::TOOL_NAME {
                return format!("The {} tool is not available in this test.", call.name);
            }
            let request = act::parse_request(&call.arguments);
            act::result_for_model(&CommandReport {
                id: 1,
                origin: CommandOrigin::Assistant,
                transcript: request.clone(),
                status: CommandStatus::Done,
                feedback: vec![Feedback::new("done", "Playing it on YouTube.", &[])],
                steps: Vec::new(),
                pending_summary: None,
                question: None,
                choices: Vec::new(),
                escalation: None,
                dry_run: true,
                dry_run_actions: vec![format!("play on YouTube: {request}")],
                model: String::new(),
                llm_calls: 1,
                usage: Usage::default(),
                latency_ms: 0,
                error: None,
            })
        })
    }

    fn capture_screen(&self) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async { Err("not in this test".to_string()) })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "calls the assistant model this machine's settings name"]
async fn assistant_live_plays_music_through_do_on_computer() {
    let mut settings = crate::command_mode::live_tests::installed_settings();
    // The tool is offered while Voice commands are on; this test is about the
    // model choosing it, whatever the switch says on this machine.
    settings.command_mode_enabled = true;
    settings.assistant_tts_enabled = false;
    let model = super::resolve_turn_model(&settings).expect("an assistant model");
    println!("assistant: {} · {}", model.provider.id, model.model);

    let messages = request_messages(
        &settings,
        &model,
        &[],
        "play believer by imagine dragons on youtube",
    );
    let hooks = Recording::default();
    let started = Instant::now();
    let answer = turn::run_tool_loop(&model, messages.with_tools, &messages.tools, &hooks, || {
        |_: &str| {}
    })
    .await
    .expect("the model answers");
    println!("answer in {:?}: {answer}", started.elapsed());

    let calls = hooks.calls.lock().unwrap().clone();
    let request = calls
        .iter()
        .find(|(name, _)| name == act::TOOL_NAME)
        .map(|(_, args)| act::parse_request(args))
        .unwrap_or_else(|| panic!("do_on_computer was not called: {calls:?}"));
    println!("do_on_computer request: {request:?}");
    let lower = request.to_lowercase();
    assert!(lower.contains("believer"), "{request}");
    assert!(lower.contains("youtube"), "{request}");
    assert!(!answer.trim().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "calls the assistant model this machine's settings name"]
async fn assistant_live_answers_a_question_without_acting() {
    let mut settings = crate::command_mode::live_tests::installed_settings();
    settings.command_mode_enabled = true;
    settings.assistant_tts_enabled = false;
    let model = super::resolve_turn_model(&settings).expect("an assistant model");
    let messages = request_messages(&settings, &model, &[], "who wrote the song Believer?");
    let hooks = Recording::default();
    let answer = turn::run_tool_loop(&model, messages.with_tools, &messages.tools, &hooks, || {
        |_: &str| {}
    })
    .await
    .expect("the model answers");
    println!("answer: {answer}");
    let calls = hooks.calls.lock().unwrap().clone();
    assert!(
        !calls.iter().any(|(name, _)| name == act::TOOL_NAME),
        "a question was acted on: {calls:?}"
    );
}
