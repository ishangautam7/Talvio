//! `text`: something typed on the phone, pasted at this computer's cursor.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{windows, Pasted, Reply, Router};

const MAX_CHARS: usize = 10_000;

pub(super) async fn handle(router: Arc<Router>, body: Value, mut reply: Reply) {
    let host = router.host();
    if !router.input_allowed() {
        return reply.fail("remote_input_off");
    }
    if !reply.take_input() {
        return reply.fail("busy");
    }
    let Some(text) = body
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| (1..=MAX_CHARS).contains(&text.chars().count()))
    else {
        return reply.fail("unsupported");
    };
    let target = match windows::target(&router, &reply, &body) {
        Ok(target) => target,
        Err(code) => return reply.fail(code),
    };
    if reply.is_cancelled() {
        return reply.fail("cancelled");
    }
    reply.progress("pasting");
    match windows::aim(&router, target.as_ref(), || host.paste(text.to_string())).await {
        Ok(Pasted::Done) => reply.ok(json!({ "pasted": true })),
        Ok(Pasted::Off) => reply.fail("paste_off"),
        Ok(Pasted::Failed) => reply.fail("paste_failed"),
        Err(code) => reply.fail(code),
    }
}
