//! The camera's flights: a short eased move from where it is to where it is going (find, a region
//! opened, the breadcrumb), the zoom eased in proportion (so a zoom in and a zoom out feel the same)
//! and the centre along a straight line, over the app's `slow` motion token (280 ms). With motion
//! reduced the duration is zero and the camera is simply there.

use std::time::{Duration, Instant};

use doze_map::Rect;

use super::camera::{Camera, MAX_ZOOM, MIN_ZOOM};

/// One move of the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Flight {
    /// Zoom and centre (map pixels) where it starts and ends.
    from: (f32, f64, f64),
    to: (f32, f64, f64),
    start: Instant,
    dur: Duration,
    /// Held at this share of the way (shots and tests), instead of the clock.
    pub(crate) held: Option<f32>,
}

/// Slow, then fast, then slow: the cubic ease-in-out.
pub(crate) fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

impl Camera {
    /// The map point at the viewport's middle.
    pub(crate) fn centre(&self) -> (f64, f64) {
        let v = self.view();
        (v.x + v.w / 2.0, v.y + v.h / 2.0)
    }

    /// The zoom and centre that show `r` whole with a margin, at most 1:1.
    pub(crate) fn framing(&self, r: Rect) -> (f32, f64, f64) {
        let (w, h) = (self.size.0.max(1.0), self.size.1.max(1.0));
        let z = ((w - 48.0) / r.w.max(1.0) as f32)
            .min((h - 48.0) / r.h.max(1.0) as f32)
            .clamp(MIN_ZOOM, 1.0);
        let c = r.center();
        (z, c.x, c.y)
    }

    /// Flies to `zoom` centred on (x, y) over `dur` (kept within the drawing `b`); at once when
    /// `dur` is zero.
    pub(crate) fn fly(&mut self, (zoom, x, y): (f32, f64, f64), b: Rect, dur: Duration) {
        let mut end = *self;
        end.flight = None;
        end.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        end.centre_on(x, y, b);
        let to = (end.zoom, end.centre().0, end.centre().1);
        if dur.is_zero() {
            *self = end;
            return;
        }
        let (cx, cy) = self.centre();
        self.flight = Some(Flight {
            from: (self.zoom, cx, cy),
            to,
            start: Instant::now(),
            dur,
            held: None,
        });
    }

    /// Where the camera is going: the flight's end, or where it is.
    pub(crate) fn target(&self) -> (f32, f64, f64) {
        match self.flight {
            Some(f) => f.to,
            None => {
                let (x, y) = self.centre();
                (self.zoom, x, y)
            }
        }
    }

    /// Moves the camera along its flight to `now`; says whether it is still flying.
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let Some(f) = self.flight else { return false };
        let t = f.held.unwrap_or_else(|| {
            let d = now.saturating_duration_since(f.start).as_secs_f32();
            d / f.dur.as_secs_f32().max(1e-3)
        });
        let e = ease(t);
        let (z0, x0, y0) = f.from;
        let (z1, x1, y1) = f.to;
        self.zoom = (z0.ln() + (z1.ln() - z0.ln()) * e).exp();
        let e = f64::from(e);
        let (x, y) = (x0 + (x1 - x0) * e, y0 + (y1 - y0) * e);
        let (w, h) = (self.size.0.max(1.0), self.size.1.max(1.0));
        self.pan = (
            x as f32 - w / self.zoom / 2.0,
            y as f32 - h / self.zoom / 2.0,
        );
        if t >= 1.0 {
            self.flight = None;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::ease;
    use crate::map::camera::Camera;
    use doze_map::Rect;

    fn cam() -> Camera {
        Camera {
            size: (1200.0, 800.0),
            zoom: 0.3,
            ..Camera::default()
        }
    }

    #[test]
    fn the_ease_starts_and_ends_slow() {
        assert!(ease(0.0).abs() < 1e-6 && (ease(1.0) - 1.0).abs() < 1e-6);
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
        assert!(ease(0.1) < 0.1 && ease(0.9) > 0.9);
    }

    #[test]
    fn a_flight_ends_where_it_was_sent_and_passes_between() {
        let b = Rect::new(0.0, 0.0, 6000.0, 3000.0);
        let mut c = cam();
        c.centre_on(600.0, 400.0, b);
        c.fly((1.0, 3000.0, 1500.0), b, Duration::from_millis(280));
        assert!(c.flight.is_some());
        if let Some(f) = c.flight.as_mut() {
            f.held = Some(0.5);
        }
        assert!(c.tick(Instant::now()));
        let (x, _) = c.centre();
        assert!(x > 700.0 && x < 2900.0, "half way: {x}");
        assert!(c.zoom > 0.3 && c.zoom < 1.0);
        if let Some(f) = c.flight.as_mut() {
            f.held = Some(1.0);
        }
        assert!(!c.tick(Instant::now()));
        assert!((c.zoom - 1.0).abs() < 1e-4);
        let (x, y) = c.centre();
        assert!((x - 3000.0).abs() < 1.0 && (y - 1500.0).abs() < 1.0);
    }

    #[test]
    fn with_no_time_the_camera_is_simply_there() {
        let b = Rect::new(0.0, 0.0, 6000.0, 3000.0);
        let mut c = cam();
        c.fly((0.8, 2000.0, 1000.0), b, Duration::ZERO);
        assert!(c.flight.is_none());
        assert!((c.zoom - 0.8).abs() < 1e-4);
        let (x, y) = c.centre();
        assert!((x - 2000.0).abs() < 1.0 && (y - 1000.0).abs() < 1.0);
    }
}
