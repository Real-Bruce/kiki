use super::geometry::PixelRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelPoint {
    pub x: i32,
    pub y: i32,
}

pub fn contains(rect: PixelRect, p: PixelPoint) -> bool {
    p.x >= rect.left && p.x < rect.right && p.y >= rect.top && p.y < rect.bottom
}

#[derive(Clone, Copy)]
enum DragKind {
    New,
    Move,
    Resize(usize),
}
#[derive(Clone, Copy)]
struct Drag {
    start: PixelPoint,
    before: Option<PixelRect>,
    kind: DragKind,
}

pub struct Selection {
    bounds: PixelRect,
    pub rect: Option<PixelRect>,
    drag: Option<Drag>,
}
impl Selection {
    pub fn new(bounds: PixelRect) -> Self {
        Self {
            bounds,
            rect: None,
            drag: None,
        }
    }
    pub fn handles(rect: PixelRect) -> [PixelPoint; 8] {
        let x = ((i64::from(rect.left) + i64::from(rect.right)) / 2) as i32;
        let y = ((i64::from(rect.top) + i64::from(rect.bottom)) / 2) as i32;
        [
            PixelPoint {
                x: rect.left,
                y: rect.top,
            },
            PixelPoint { x, y: rect.top },
            PixelPoint {
                x: rect.right,
                y: rect.top,
            },
            PixelPoint { x: rect.right, y },
            PixelPoint {
                x: rect.right,
                y: rect.bottom,
            },
            PixelPoint { x, y: rect.bottom },
            PixelPoint {
                x: rect.left,
                y: rect.bottom,
            },
            PixelPoint { x: rect.left, y },
        ]
    }
    fn clamp(&self, p: PixelPoint) -> PixelPoint {
        PixelPoint {
            x: p.x.clamp(self.bounds.left, self.bounds.right),
            y: p.y.clamp(self.bounds.top, self.bounds.bottom),
        }
    }
    pub fn begin(&mut self, p: PixelPoint, radius: i32) {
        let p = self.clamp(p);
        let kind = if let Some(rect) = self.rect {
            if let Some(index) = Self::handles(rect).iter().position(|h| {
                (i64::from(h.x) - i64::from(p.x)).abs() <= i64::from(radius)
                    && (i64::from(h.y) - i64::from(p.y)).abs() <= i64::from(radius)
            }) {
                DragKind::Resize(index)
            } else if contains(rect, p) {
                DragKind::Move
            } else {
                DragKind::New
            }
        } else {
            DragKind::New
        };
        self.drag = Some(Drag {
            start: p,
            before: self.rect,
            kind,
        });
    }
    pub fn update(&mut self, p: PixelPoint) {
        let Some(drag) = self.drag else { return };
        let p = self.clamp(p);
        let rect = match (drag.kind, drag.before) {
            (DragKind::Move, Some(r)) => {
                let dx = (i64::from(p.x) - i64::from(drag.start.x)).clamp(
                    i64::from(self.bounds.left) - i64::from(r.left),
                    i64::from(self.bounds.right) - i64::from(r.right),
                );
                let dy = (i64::from(p.y) - i64::from(drag.start.y)).clamp(
                    i64::from(self.bounds.top) - i64::from(r.top),
                    i64::from(self.bounds.bottom) - i64::from(r.bottom),
                );
                PixelRect {
                    left: (i64::from(r.left) + dx) as i32,
                    right: (i64::from(r.right) + dx) as i32,
                    top: (i64::from(r.top) + dy) as i32,
                    bottom: (i64::from(r.bottom) + dy) as i32,
                }
            }
            (DragKind::Resize(i), Some(mut r)) => {
                if [0, 6, 7].contains(&i) {
                    r.left = p.x;
                }
                if [2, 3, 4].contains(&i) {
                    r.right = p.x;
                }
                if [0, 1, 2].contains(&i) {
                    r.top = p.y;
                }
                if [4, 5, 6].contains(&i) {
                    r.bottom = p.y;
                }
                normalize(
                    PixelPoint {
                        x: r.left,
                        y: r.top,
                    },
                    PixelPoint {
                        x: r.right,
                        y: r.bottom,
                    },
                )
            }
            _ => normalize(drag.start, p),
        };
        self.rect = rect.size().map(|_| rect);
    }
    pub fn finish(&mut self, p: PixelPoint, window: Option<PixelRect>) {
        if let Some(drag) = self.drag {
            let click = (i64::from(p.x) - i64::from(drag.start.x)).abs() < 3
                && (i64::from(p.y) - i64::from(drag.start.y)).abs() < 3;
            if matches!(drag.kind, DragKind::New) && click {
                self.rect = window.and_then(|r| self.bounds.intersection(r));
            } else {
                self.update(p);
            }
        }
        self.drag = None;
    }
    pub fn cancel_drag(&mut self) -> bool {
        if let Some(drag) = self.drag.take() {
            self.rect = drag.before;
            true
        } else {
            false
        }
    }
    pub fn active(&self) -> bool {
        self.drag.is_some()
    }
    pub fn fullscreen(&mut self, screen: PixelRect) {
        self.drag = None;
        self.rect = self.bounds.intersection(screen);
    }
}
fn normalize(a: PixelPoint, b: PixelPoint) -> PixelRect {
    PixelRect {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    }
}

/// Entire toolbar stays on one actual display, below, above, then safely inside it.
pub fn toolbar(rect: PixelRect, screen: PixelRect, width: i32, height: i32, gap: i32) -> PixelRect {
    let width = i64::from(width.max(1))
        .min(i64::from(screen.right) - i64::from(screen.left))
        .max(1) as i32;
    let height = i64::from(height.max(1))
        .min(i64::from(screen.bottom) - i64::from(screen.top))
        .max(1) as i32;
    let left = ((i64::from(rect.left) + i64::from(rect.right) - i64::from(width)) / 2).clamp(
        i64::from(screen.left),
        i64::from(screen.right) - i64::from(width),
    );
    let below = i64::from(rect.bottom) + i64::from(gap);
    let above = i64::from(rect.top) - i64::from(gap) - i64::from(height);
    let top = if below >= i64::from(screen.top)
        && below + i64::from(height) <= i64::from(screen.bottom)
    {
        below
    } else if above >= i64::from(screen.top)
        && above + i64::from(height) <= i64::from(screen.bottom)
    {
        above
    } else {
        (i64::from(screen.bottom) - i64::from(height) - i64::from(gap)).max(i64::from(screen.top))
    };
    PixelRect {
        left: left as i32,
        top: top as i32,
        right: (left + i64::from(width)) as i32,
        bottom: (top + i64::from(height)) as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reverse_drag_move_resize_and_cancel() {
        let bounds = PixelRect {
            left: -1920,
            top: -1080,
            right: 1920,
            bottom: 1080,
        };
        let mut s = Selection::new(bounds);
        s.begin(PixelPoint { x: 600, y: 500 }, 5);
        s.update(PixelPoint { x: -300, y: -200 });
        s.finish(PixelPoint { x: -300, y: -200 }, None);
        assert_eq!(
            s.rect,
            Some(PixelRect {
                left: -300,
                top: -200,
                right: 600,
                bottom: 500
            })
        );
        s.begin(PixelPoint { x: 0, y: 0 }, 5);
        s.update(PixelPoint { x: 1900, y: 1000 });
        assert_eq!(s.rect.unwrap().right, 1920);
        assert!(s.cancel_drag());
        assert_eq!(s.rect.unwrap().left, -300);
        s.begin(PixelPoint { x: 600, y: 500 }, 5);
        s.finish(PixelPoint { x: 800, y: 700 }, None);
        assert_eq!(s.rect.unwrap().size(), Some((1100, 900)));
    }
    #[test]
    fn toolbar_avoids_bottom_and_remains_on_negative_display() {
        let screen = PixelRect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        let rect = PixelRect {
            left: -50,
            top: 900,
            right: 0,
            bottom: 1080,
        };
        let bar = toolbar(rect, screen, 650, 60, 12);
        assert_eq!(bar.bottom, 888);
        assert_eq!(bar.right, 0);
        let full = toolbar(screen, screen, 650, 60, 12);
        assert!(full.top >= 0 && full.bottom <= 1080);
    }

    #[test]
    fn all_handles_preserve_opposite_edges_and_allow_crossing() {
        let bounds = PixelRect {
            left: -300,
            top: -200,
            right: 300,
            bottom: 200,
        };
        let original = PixelRect {
            left: -100,
            top: -80,
            right: 100,
            bottom: 80,
        };
        let cases = [
            (0, PixelPoint { x: -120, y: -90 }, [-120, -90, 100, 80]),
            (1, PixelPoint { x: 0, y: -90 }, [-100, -90, 100, 80]),
            (2, PixelPoint { x: 120, y: -90 }, [-100, -90, 120, 80]),
            (3, PixelPoint { x: 120, y: 0 }, [-100, -80, 120, 80]),
            (4, PixelPoint { x: 120, y: 90 }, [-100, -80, 120, 90]),
            (5, PixelPoint { x: 0, y: 90 }, [-100, -80, 100, 90]),
            (6, PixelPoint { x: -120, y: 90 }, [-120, -80, 100, 90]),
            (7, PixelPoint { x: -120, y: 0 }, [-120, -80, 100, 80]),
            (0, PixelPoint { x: 150, y: 120 }, [100, 80, 150, 120]),
        ];
        for (index, end, [left, top, right, bottom]) in cases {
            let mut selection = Selection::new(bounds);
            selection.rect = Some(original);
            selection.begin(Selection::handles(original)[index], 5);
            selection.finish(end, None);
            assert_eq!(
                selection.rect,
                Some(PixelRect {
                    left,
                    top,
                    right,
                    bottom
                })
            );
            assert!(!selection.active());
        }
    }

    #[test]
    fn window_click_clips_to_snapshot_and_cancel_restores_previous_selection() {
        let bounds = PixelRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let window = PixelRect {
            left: -10,
            top: -20,
            right: 900,
            bottom: 700,
        };
        let point = PixelPoint { x: 400, y: 300 };
        let mut selection = Selection::new(bounds);
        selection.begin(point, 6);
        selection.finish(point, Some(window));
        let clipped = Some(PixelRect {
            left: 0,
            top: 0,
            right: 900,
            bottom: 700,
        });
        assert_eq!(selection.rect, clipped);
        selection.begin(PixelPoint { x: 1100, y: 800 }, 6);
        selection.update(PixelPoint { x: 1800, y: 1000 });
        assert!(selection.cancel_drag());
        assert_eq!(selection.rect, clipped);
        assert!(!selection.cancel_drag());
        selection.begin(point, 6);
        selection.update(PixelPoint { x: 1500, y: 1000 });
        assert!(selection.cancel_drag());
        assert_eq!(selection.rect, clipped);
    }

    #[test]
    fn one_pixel_selection_remains_resizable_at_screen_edge() {
        let bounds = PixelRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let mut selection = Selection::new(bounds);
        selection.rect = Some(PixelRect {
            left: 1910,
            top: 1070,
            right: 1920,
            bottom: 1080,
        });
        selection.begin(PixelPoint { x: 1910, y: 1070 }, 2);
        selection.finish(PixelPoint { x: 1919, y: 1079 }, None);
        assert_eq!(selection.rect.and_then(PixelRect::size), Some((1, 1)));
        selection.begin(PixelPoint { x: 1919, y: 1079 }, 2);
        selection.finish(PixelPoint { x: 1800, y: 900 }, None);
        assert_eq!(selection.rect.and_then(PixelRect::size), Some((120, 180)));
    }
}
