//! `doze-app shot --screen map --map …`: the Map set up for a shot through the calls its keys and clicks make.

use std::time::{Duration, Instant};

use gpui::{Context, Window};

use super::{Lens, MapView};

impl MapView {
    /// Applies a shot's `--map` spec (see `cli.rs`), in order.
    pub(crate) fn stage(&mut self, spec: &str, window: &mut Window, cx: &mut Context<Self>) {
        for part in spec.split(',').map(str::trim) {
            let (what, arg) = part.split_once(':').unwrap_or((part, ""));
            match what {
                "lines" => self.all_lines = true,
                "tile" => self.open_list(window, cx),
                "trouble" => self.on_trouble(cx),
                "hood" => {
                    let centre = self.lit.and_then(|c| self.layout.card(c)).map(|c| c.name.clone());
                    if let Some(centre) = centre {
                        let hops = arg.parse().unwrap_or(2);
                        self.enter(Lens::Hood { centre, hops }, cx);
                    }
                }
                "fly" => self.held_flight(arg.parse().unwrap_or(0.5)),
                "region" => {
                    if let Some(k) = self.layout.regions.iter().position(|r| r.name == arg) {
                        self.open_region(k, cx);
                        self.hold(1.0);
                    }
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// Find's flight to the selection from the whole map, held at `p` of the way.
    fn held_flight(&mut self, p: f32) {
        let Some(c) = self.lit.and_then(|c| self.layout.card(c)) else {
            return;
        };
        let ctr = c.rect.center();
        self.cam.fit(&self.layout);
        self.cam.fly(
            (1.0, ctr.x, ctr.y),
            self.layout.bounds,
            Duration::from_millis(280),
        );
        self.hold(p);
    }

    fn hold(&mut self, p: f32) {
        if let Some(f) = self.cam.flight.as_mut() {
            f.held = Some(p);
        }
        self.cam.tick(Instant::now());
    }
}
