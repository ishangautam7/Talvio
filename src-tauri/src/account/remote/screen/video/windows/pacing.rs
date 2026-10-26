//! When to encode, and at what size. Pure decisions with the clock passed in,
//! so every branch is tested without capturing anything.

use std::time::{Duration, Instant};

/// The smallest side the encoders are given. Both refused a minimized
/// window's 146x28; a smaller picture is letterboxed into this.
pub const MIN_SIDE: u32 = 128;

/// The size to encode a `width` x `height` picture at: at most `max_long_side`
/// on its long side, aspect kept, both even (4:2:0 needs even sizes), and no
/// side below [`MIN_SIDE`] (a narrower picture is letterboxed).
pub fn output_size(width: u32, height: u32, max_long_side: u32) -> (u32, u32) {
    let (width, height) = (width.max(2), height.max(2));
    let long = width.max(height);
    let max_long = max_long_side.max(MIN_SIDE);
    let (w, h) = if long <= max_long {
        (width, height)
    } else {
        let scale = f64::from(max_long) / f64::from(long);
        (
            (f64::from(width) * scale).round() as u32,
            (f64::from(height) * scale).round() as u32,
        )
    };
    (even(w).max(MIN_SIDE), even(h).max(MIN_SIDE))
}

fn even(value: u32) -> u32 {
    (value & !1).max(2)
}

/// Where a `src_w` x `src_h` picture goes inside a `dst_w` x `dst_h` frame:
/// as large as fits with its aspect kept, centred. Used for the moments while
/// a window is being resized and the encoder still runs at the old size, so
/// the picture is letterboxed instead of stretched. `(left, top, right,
/// bottom)`.
pub fn fit_rect(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> (u32, u32, u32, u32) {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return (0, 0, dst_w, dst_h);
    }
    // Compare src_w/src_h with dst_w/dst_h without floating point.
    let (w, h) = if u64::from(src_w) * u64::from(dst_h) >= u64::from(src_h) * u64::from(dst_w) {
        let h = (u64::from(dst_w) * u64::from(src_h) / u64::from(src_w)) as u32;
        (dst_w, h.clamp(1, dst_h))
    } else {
        let w = (u64::from(dst_h) * u64::from(src_w) / u64::from(src_h)) as u32;
        (w.clamp(1, dst_w), dst_h)
    };
    let left = ((dst_w - w) / 2) & !1;
    let top = ((dst_h - h) / 2) & !1;
    (left, top, left + w, top + h)
}

/// What the capture loop does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Take the newest captured frame and encode it.
    EncodeNew,
    /// Encode the last picture again (keepalive, a requested keyframe, or a
    /// letterboxed picture that can now be shown at its own size).
    Repeat,
    /// Nothing to do until this moment, unless something arrives first.
    Wait(Instant),
}

/// Paces the encoder: at most one frame per `min_interval`, the last picture
/// again after `keepalive` without a new one, and a repeat as soon as the rate
/// allows when one is wanted.
pub struct Pacer {
    min_interval: Duration,
    keepalive: Duration,
    started: Instant,
    last_sent: Option<Instant>,
    /// The frame slot the last frame went out in. Slots are spaced
    /// `min_interval` apart on an ideal grid, not from the moment the last
    /// frame actually went out: a thread's wake-up lands up to a timer tick
    /// late, and pacing from the late moment turned 30 fps into 26.
    slot: Option<Instant>,
    /// The capture has a newer frame than the one last encoded.
    frame_waiting: bool,
    /// Encode the last picture again as soon as the rate allows.
    repeat_wanted: bool,
    /// Encode the last picture again at this moment (a resize settling).
    repeat_at: Option<Instant>,
}

impl Pacer {
    pub fn new(max_fps: u32, keepalive: Duration, started: Instant) -> Self {
        Self {
            min_interval: Duration::from_secs(1) / max_fps.clamp(1, 240),
            // A keepalive shorter than a frame interval would be a frame rate.
            keepalive: keepalive.max(Duration::from_millis(100)),
            started,
            last_sent: None,
            slot: None,
            frame_waiting: false,
            repeat_wanted: false,
            repeat_at: None,
        }
    }

    pub fn frame_arrived(&mut self) {
        self.frame_waiting = true;
    }

    pub fn want_repeat(&mut self) {
        self.repeat_wanted = true;
    }

    pub fn repeat_at(&mut self, at: Instant) {
        self.repeat_at = Some(self.repeat_at.map_or(at, |old| old.min(at)));
    }

    /// A frame went out at `now` (new or repeated).
    pub fn sent(&mut self, now: Instant, was_new: bool) {
        self.last_sent = Some(now);
        // The next slot on the grid when this frame is within a slot of it;
        // after a pause, the grid starts again from now.
        self.slot = Some(match self.slot {
            Some(slot) => {
                let ideal = slot + self.min_interval;
                if now >= ideal && now < ideal + self.min_interval {
                    ideal
                } else {
                    now
                }
            }
            None => now,
        });
        if was_new {
            self.frame_waiting = false;
        }
        self.repeat_wanted = false;
        if self.repeat_at.is_some_and(|at| at <= now) {
            self.repeat_at = None;
        }
    }

    /// The new frame turned out not to be there (the pool was empty).
    pub fn nothing_new(&mut self) {
        self.frame_waiting = false;
    }

    pub fn next(&self, now: Instant) -> Step {
        let Some(last) = self.last_sent else {
            // Nothing sent yet: the first frame goes out the moment there is
            // one, and a picture is sent anyway after the keepalive (a window
            // minimized from the start still shows the phone something).
            let due = self.started + self.keepalive;
            return if self.frame_waiting {
                Step::EncodeNew
            } else if self.repeat_wanted || now >= due {
                Step::Repeat
            } else {
                Step::Wait(due)
            };
        };
        let rate_ok = self.slot.unwrap_or(last) + self.min_interval;
        if self.frame_waiting {
            return if now >= rate_ok {
                Step::EncodeNew
            } else {
                Step::Wait(rate_ok)
            };
        }
        let mut due = last + self.keepalive;
        if self.repeat_wanted {
            due = due.min(rate_ok);
        }
        if let Some(at) = self.repeat_at {
            due = due.min(at.max(rate_ok));
        }
        if now >= due {
            Step::Repeat
        } else {
            Step::Wait(due)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn output_size_scales_the_long_side_and_keeps_sizes_even() {
        assert_eq!(output_size(1920, 1080, 1920), (1920, 1080));
        assert_eq!(output_size(3840, 2160, 1920), (1920, 1080));
        assert_eq!(output_size(2160, 3840, 1920), (1080, 1920));
        assert_eq!(output_size(2560, 1440, 1280), (1280, 720));
        // Odd window sizes become even, smaller ones are not enlarged.
        assert_eq!(output_size(801, 601, 1920), (800, 600));
        assert_eq!(output_size(3001, 1001, 1920), (1920, 640));
        // Tiny pictures (a minimized window) get a size encoders take.
        assert_eq!(output_size(146, 28, 1920), (146, 128));
        assert_eq!(output_size(1, 1, 1920), (128, 128));
        assert_eq!(output_size(0, 0, 0), (128, 128));
        assert_eq!(output_size(4000, 100, 1920), (1920, 128));
        for (w, h) in [(1366, 768), (1279, 1023), (5120, 1440), (333, 4001)] {
            let (ow, oh) = output_size(w, h, 1280);
            assert!(ow % 2 == 0 && oh % 2 == 0 && ow.max(oh) <= 1280, "{w}x{h}");
        }
    }

    #[test]
    fn fit_rect_letterboxes_and_pillarboxes() {
        assert_eq!(fit_rect(1920, 1080, 1920, 1080), (0, 0, 1920, 1080));
        // Wider picture: bars top and bottom.
        assert_eq!(fit_rect(2000, 500, 1000, 1000), (0, 374, 1000, 624));
        // Taller picture: bars left and right.
        assert_eq!(fit_rect(500, 1000, 1000, 1000), (250, 0, 750, 1000));
        assert_eq!(fit_rect(0, 10, 100, 100), (0, 0, 100, 100));
    }

    #[test]
    fn first_frame_goes_out_at_once() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(30, ms(2000), t0);
        assert_eq!(pacer.next(t0), Step::Wait(t0 + ms(2000)));
        assert_eq!(pacer.next(t0 + ms(2000)), Step::Repeat);
        pacer.frame_arrived();
        assert_eq!(pacer.next(t0), Step::EncodeNew);
    }

    #[test]
    fn frames_are_held_to_the_rate() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(20, ms(2000), t0);
        pacer.frame_arrived();
        pacer.sent(t0, true);
        pacer.frame_arrived();
        assert_eq!(pacer.next(t0 + ms(10)), Step::Wait(t0 + ms(50)));
        assert_eq!(pacer.next(t0 + ms(50)), Step::EncodeNew);
    }

    #[test]
    fn late_wake_ups_do_not_lower_the_rate() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(20, ms(2000), t0);
        pacer.frame_arrived();
        pacer.sent(t0, true);
        // Each frame goes out 10 ms after it was allowed (a coarse timer).
        let mut at = t0;
        for n in 1..=10u32 {
            pacer.frame_arrived();
            let allowed = t0 + ms(50) * n;
            assert_eq!(pacer.next(at), Step::Wait(allowed));
            at = allowed + ms(10);
            assert_eq!(pacer.next(at), Step::EncodeNew);
            pacer.sent(at, true);
        }
        // Ten frames in 510 ms: the rate held.
        assert_eq!(at, t0 + ms(510));
        // After a pause the grid restarts from the next frame.
        let later = at + ms(1000);
        pacer.frame_arrived();
        assert_eq!(pacer.next(later), Step::EncodeNew);
        pacer.sent(later, true);
        pacer.frame_arrived();
        assert_eq!(pacer.next(later + ms(1)), Step::Wait(later + ms(50)));
    }

    #[test]
    fn a_still_picture_is_repeated_after_the_keepalive() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(30, ms(2000), t0);
        pacer.frame_arrived();
        pacer.sent(t0, true);
        assert_eq!(pacer.next(t0 + ms(100)), Step::Wait(t0 + ms(2000)));
        assert_eq!(pacer.next(t0 + ms(2000)), Step::Repeat);
        pacer.sent(t0 + ms(2000), false);
        assert_eq!(pacer.next(t0 + ms(2001)), Step::Wait(t0 + ms(4000)));
    }

    #[test]
    fn a_wanted_repeat_waits_only_for_the_rate() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(10, ms(2000), t0);
        pacer.frame_arrived();
        pacer.sent(t0, true);
        pacer.want_repeat();
        assert_eq!(pacer.next(t0 + ms(30)), Step::Wait(t0 + ms(100)));
        assert_eq!(pacer.next(t0 + ms(100)), Step::Repeat);
        pacer.sent(t0 + ms(100), false);
        assert_eq!(pacer.next(t0 + ms(150)), Step::Wait(t0 + ms(2100)));
    }

    #[test]
    fn a_scheduled_repeat_fires_once() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(30, ms(2000), t0);
        pacer.frame_arrived();
        pacer.sent(t0, true);
        pacer.repeat_at(t0 + ms(250));
        assert_eq!(pacer.next(t0 + ms(40)), Step::Wait(t0 + ms(250)));
        assert_eq!(pacer.next(t0 + ms(250)), Step::Repeat);
        pacer.sent(t0 + ms(250), false);
        assert_eq!(pacer.next(t0 + ms(300)), Step::Wait(t0 + ms(2250)));
    }

    #[test]
    fn a_repeat_before_any_frame_is_allowed() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(30, ms(2000), t0);
        pacer.want_repeat();
        assert_eq!(pacer.next(t0), Step::Repeat);
    }

    #[test]
    fn an_empty_pool_clears_the_waiting_frame() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(30, ms(2000), t0);
        pacer.frame_arrived();
        pacer.nothing_new();
        assert_eq!(pacer.next(t0), Step::Wait(t0 + ms(2000)));
    }
}
