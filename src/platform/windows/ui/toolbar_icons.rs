//! Shared native geometry from the approved A design, centered on its 24 DIP grid.
use crate::core::{geometry::PixelRect, selection::PixelPoint};
use crate::platform::windows::{
    editor::toolbar::{
        BUTTON_HEIGHT, COPY, LONG_CAPTURE, Layout, MORE, MOSAIC, PICKER, SEQUENCE, UNDO,
    },
    ui::palette,
};
use windows::{
    Win32::Graphics::Direct2D::{Common::*, *},
    core::Result,
};
use windows_numerics::{Matrix3x2, Vector2};

pub(in crate::platform::windows) struct ToolbarIcons {
    paths: [Option<ID2D1PathGeometry>; 10],
    picker: ID2D1PathGeometry,
    stroke: ID2D1StrokeStyle,
    long_capture: ID2D1PathGeometry,
}

type Point = (f32, f32);

#[derive(Clone, Copy)]
enum Segment {
    Line(Point),
    Curve(Point, Point, Point),
}
use Segment::{Curve as C, Line as L};

struct Figure(Point, &'static [Segment], bool);

const RECTANGLE: &[Figure] = &[Figure(
    (-5.8, -7.75),
    &[
        L((5.8, -7.75)),
        C((7.291, -7.75), (8.5, -6.541), (8.5, -5.05)),
        L((8.5, 5.05)),
        C((8.5, 6.541), (7.291, 7.75), (5.8, 7.75)),
        L((-5.8, 7.75)),
        C((-7.291, 7.75), (-8.5, 6.541), (-8.5, 5.05)),
        L((-8.5, -5.05)),
        C((-8.5, -6.541), (-7.291, -7.75), (-5.8, -7.75)),
    ],
    true,
)];
const ARROW: &[Figure] = &[
    Figure((-7.5, 7.5), &[L((7.5, -7.5))], false),
    Figure((-1.5, -7.5), &[L((7.5, -7.5)), L((7.5, 1.5))], false),
];
const PEN: &[Figure] = &[Figure(
    (-8.6, 5.8),
    &[
        C((-6.3, 5.8), (-5.2, -5.8), (-1.7, -5.8)),
        C((2.1, -5.8), (0.2, 5.6), (3.5, 5.6)),
        C((5.5, 5.6), (7., 2.1), (8.6, 2.1)),
    ],
    false,
)];
const TEXT: &[Figure] = &[
    Figure(
        (-7., -5.),
        &[L((-7., -7.)), L((7., -7.)), L((7., -5.))],
        false,
    ),
    Figure((0., -7.), &[L((0., 7.))], false),
    Figure((-3.5, 7.), &[L((3.5, 7.))], false),
];
const UNDO_ICON: &[Figure] = &[
    Figure((-3.5, -7.5), &[L((-8.5, -2.5)), L((-3.5, 2.5))], false),
    Figure(
        (-8.5, -2.5),
        &[
            L((2.7, -2.5)),
            C((5.572, -2.5), (7.9, -0.172), (7.9, 2.7)),
            C((7.9, 5.572), (5.572, 7.9), (2.7, 7.9)),
            L((0., 7.9)),
        ],
        false,
    ),
];
const PIN: &[Figure] = &[
    Figure(
        (0.4, -7.5),
        &[
            L((1.6, -8.7)),
            C((1.987, -9.087), (2.613, -9.087), (3., -8.7)),
            L((8.7, -3.)),
            C((9.087, -2.613), (9.087, -1.987), (8.7, -1.6)),
            L((7.5, -0.4)),
            L((5., -0.8)),
            L((0.8, 3.4)),
            L((0.2, 6.6)),
            L((-6.6, -0.2)),
            L((-3.4, -0.8)),
            L((0.8, -5.)),
            L((0.4, -7.5)),
        ],
        true,
    ),
    Figure((-3.2, 3.2), &[L((-8.4, 8.4))], false),
];
const SAVE: &[Figure] = &[
    Figure(
        (-6.5, -8.5),
        &[
            L((4.2, -8.5)),
            L((8.5, -4.2)),
            L((8.5, 6.5)),
            C((8.5, 7.605), (7.605, 8.5), (6.5, 8.5)),
            L((-6.5, 8.5)),
            C((-7.605, 8.5), (-8.5, 7.605), (-8.5, 6.5)),
            L((-8.5, -6.5)),
            C((-8.5, -7.605), (-7.605, -8.5), (-6.5, -8.5)),
        ],
        true,
    ),
    Figure(
        (-4.5, -8.5),
        &[L((-4.5, -3.)), L((3.5, -3.)), L((3.5, -8.5))],
        false,
    ),
    Figure(
        (-4.5, 8.5),
        &[L((-4.5, 2.1)), L((4.5, 2.1)), L((4.5, 8.5))],
        false,
    ),
];
const COPY_ICON: &[Figure] = &[
    Figure(
        (-1.7, -8.5),
        &[
            L((6.2, -8.5)),
            C((7.47, -8.5), (8.5, -7.47), (8.5, -6.2)),
            L((8.5, 1.7)),
            C((8.5, 2.97), (7.47, 4.), (6.2, 4.)),
            L((-1.7, 4.)),
            C((-2.97, 4.), (-4., 2.97), (-4., 1.7)),
            L((-4., -6.2)),
            C((-4., -7.47), (-2.97, -8.5), (-1.7, -8.5)),
        ],
        true,
    ),
    Figure(
        (-7.5, -3.5),
        &[
            C((-8.12, -3.14), (-8.5, -2.516), (-8.5, -1.8)),
            L((-8.5, 6.5)),
            C((-8.5, 7.605), (-7.605, 8.5), (-6.5, 8.5)),
            L((1.8, 8.5)),
            C((2.516, 8.5), (3.14, 8.12), (3.5, 7.5)),
        ],
        false,
    ),
];
const CLOSE: &[Figure] = &[
    Figure((-6.2, -6.2), &[L((6.2, 6.2))], false),
    Figure((6.2, -6.2), &[L((-6.2, 6.2))], false),
];
const EYEDROPPER: &[Figure] = &[
    Figure(
        (0.9, -5.6),
        &[
            L((3.4, -8.1)),
            C((4.505, -9.205), (6.295, -9.205), (7.4, -8.1)),
            C((8.505, -6.995), (8.505, -5.205), (7.4, -4.1)),
            L((4.9, -1.6)),
        ],
        false,
    ),
    Figure((-0.6, -7.), &[L((6.6, 0.2))], false),
    Figure(
        (0.8, -4.9),
        &[
            L((-7., 2.9)),
            C((-7.6, 3.5), (-7.9, 4.2), (-8., 5.)),
            L((-8.5, 8.5)),
            L((-5., 8.)),
            C((-4.2, 7.9), (-3.5, 7.6), (-2.9, 7.)),
            L((4.9, -0.8)),
        ],
        false,
    ),
];

impl ToolbarIcons {
    pub fn new(factory: &ID2D1Factory) -> Result<Self> {
        // SAFETY: factory and sinks stay on the render thread. Each figure and sink is closed before use.
        unsafe {
            let stroke = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 1.,
                    ..Default::default()
                },
                None,
            )?;
            let path = |figures: &[Figure]| -> Result<ID2D1PathGeometry> {
                let geometry = factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                let point = |(x, y): Point| Vector2 { X: x, Y: y };
                for Figure(start, segments, closed) in figures {
                    sink.BeginFigure(point(*start), D2D1_FIGURE_BEGIN_HOLLOW);
                    for segment in *segments {
                        match *segment {
                            L(p) => sink.AddLine(point(p)),
                            C(a, b, c) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                                point1: point(a),
                                point2: point(b),
                                point3: point(c),
                            }),
                        }
                    }
                    sink.EndFigure(if *closed {
                        D2D1_FIGURE_END_CLOSED
                    } else {
                        D2D1_FIGURE_END_OPEN
                    });
                }
                sink.Close()?;
                Ok(geometry)
            };
            Ok(Self {
                paths: [
                    Some(path(RECTANGLE)?),
                    Some(path(ARROW)?),
                    Some(path(PEN)?),
                    Some(path(TEXT)?),
                    Some(path(UNDO_ICON)?),
                    None,
                    Some(path(PIN)?),
                    Some(path(SAVE)?),
                    Some(path(COPY_ICON)?),
                    Some(path(CLOSE)?),
                ],
                picker: path(EYEDROPPER)?,
                long_capture: path(&[
                    Figure((-5.5, -5.5), &[L((0., 0.)), L((5.5, -5.5))], false),
                    Figure((-5.5, 1.), &[L((0., 6.5)), L((5.5, 1.))], false),
                ])?,
                stroke,
            })
        }
    }

    pub fn draw(
        &self,
        target: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        index: usize,
        center: Vector2,
        scale: f32,
    ) {
        // SAFETY: retained render-thread geometry and stroke style. Restore the caller's transform.
        unsafe {
            if index == MORE {
                for offset in [-7., 0., 7.] {
                    target.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 {
                                X: center.X + offset * scale,
                                ..center
                            },
                            radiusX: 1.35 * scale,
                            radiusY: 1.35 * scale,
                        },
                        brush,
                    );
                }
                return;
            }
            if index == MOSAIC {
                let opacity = brush.GetOpacity();
                for row in 0..3 {
                    for column in 0..3 {
                        let left = center.X + (column as f32 * 5.5 - 8.) * scale;
                        let top = center.Y + (row as f32 * 5.5 - 8.) * scale;
                        let tile = D2D_RECT_F {
                            left,
                            top,
                            right: left + 5. * scale,
                            bottom: top + 5. * scale,
                        };
                        brush
                            .SetOpacity(opacity * if (row + column) % 2 == 0 { 0.9 } else { 0.32 });
                        target.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: tile,
                                radiusX: 0.35 * scale,
                                radiusY: 0.35 * scale,
                            },
                            brush,
                        );
                    }
                }
                brush.SetOpacity(opacity);
                return;
            }
            if index == SEQUENCE {
                let radius = 8.7 * scale;
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: center,
                        radiusX: radius,
                        radiusY: radius,
                    },
                    brush,
                    1.65 * scale,
                    None,
                );
                let point = |x: f32, y: f32| Vector2 {
                    X: center.X + x * scale,
                    Y: center.Y + y * scale,
                };
                for (a, b) in [
                    (point(-2.3, -1.9), point(0.3, -4.3)),
                    (point(0.3, -4.3), point(0.3, 4.3)),
                    (point(-2.1, 4.3), point(2.7, 4.3)),
                ] {
                    target.DrawLine(a, b, brush, 1.65 * scale, &self.stroke);
                }
                return;
            }
            let path = if index == LONG_CAPTURE {
                Some(&self.long_capture)
            } else if index == PICKER {
                Some(&self.picker)
            } else {
                index
                    .checked_sub(1)
                    .and_then(|i| self.paths.get(i))
                    .and_then(Option::as_ref)
            };
            let Some(path) = path else {
                return;
            };
            let mut prior = Matrix3x2::default();
            target.GetTransform(&mut prior);
            target.SetTransform(
                &(Matrix3x2 {
                    M11: scale,
                    M22: scale,
                    M31: center.X,
                    M32: center.Y,
                    M12: 0.,
                    M21: 0.,
                } * prior),
            );
            target.DrawGeometry(path, brush, 1.65, &self.stroke);
            target.SetTransform(&prior);
        }
    }
}

#[derive(Default)]
pub(in crate::platform::windows) struct BarState {
    pub selected: usize,
    pub hover: Option<usize>,
    pub pressed: Option<usize>,
    pub focus: usize,
    pub can_undo: bool,
    pub busy: bool,
}

pub(in crate::platform::windows) struct BarPaint<'a> {
    pub target: &'a ID2D1RenderTarget,
    pub brush: &'a ID2D1SolidColorBrush,
    pub icons: &'a ToolbarIcons,
    pub dark: bool,
}

impl BarPaint<'_> {
    pub fn draw(&self, layout: &Layout, origin: PixelPoint, state: BarState) {
        let local = |bounds: PixelRect| D2D_RECT_F {
            left: (i64::from(bounds.left) - i64::from(origin.x)) as f32,
            top: (i64::from(bounds.top) - i64::from(origin.y)) as f32,
            right: (i64::from(bounds.right) - i64::from(origin.x)) as f32,
            bottom: (i64::from(bounds.bottom) - i64::from(origin.y)) as f32,
        };
        let bar = local(layout.toolbar);
        let scale = layout.scale;
        let dark = self.dark;
        let radius = 10. * scale;
        let panel = D2D1_ROUNDED_RECT {
            rect: bar,
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: target and brush are owned by the current settings or preview
        // paint pass. Geometry is consumed synchronously on this UI thread.
        unsafe {
            self.brush.SetColor(&color(palette::background(dark), 1.));
            self.target.FillRoundedRectangle(&panel, self.brush);
            self.brush.SetColor(&color(palette::divider(dark), 1.));
            self.target
                .DrawRoundedRectangle(&panel, self.brush, 1., None);
            for center in layout.dividers() {
                let x = (i64::from(center) - i64::from(origin.x)) as f32;
                self.target.FillRectangle(
                    &D2D_RECT_F {
                        left: x - 0.45 * scale,
                        right: x + 0.45 * scale,
                        top: bar.top + 14. * scale,
                        bottom: bar.bottom - 14. * scale,
                    },
                    self.brush,
                );
            }
            for button in layout.buttons() {
                let id = button.id;
                let b = local(button.bounds);
                let inset = (BUTTON_HEIGHT - 36.) * scale / 2.;
                let face = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        top: b.top + inset,
                        bottom: b.bottom - inset,
                        ..b
                    },
                    radiusX: 6. * scale,
                    radiusY: 6. * scale,
                };
                let active = !state.busy && (id != UNDO || state.can_undo);
                let over = state.hover == Some(id);
                let selected = id == state.selected
                    || id == MORE && layout.overflow().any(|id| id == state.selected);
                let down = over && active && state.pressed == Some(id);
                // Copy keeps the hover treatment throughout the click.
                let background = if down && id != COPY {
                    Some(palette::pressed(dark))
                } else if over && active {
                    Some(palette::hover(dark))
                } else if selected {
                    Some(palette::selected(dark))
                } else {
                    None
                };
                if let Some(background) = background {
                    self.brush.SetColor(&color(background, 1.));
                    self.target.FillRoundedRectangle(&face, self.brush);
                }
                let foreground = if !active {
                    color(palette::muted(dark), 0.58)
                } else if selected {
                    color(palette::accent(dark), 1.)
                } else {
                    color(palette::foreground(dark), 1.)
                };
                self.brush.SetColor(&foreground);
                let center = Vector2 {
                    X: (b.left + b.right) / 2.,
                    Y: (b.top + b.bottom) / 2.,
                };
                self.icons.draw(self.target, self.brush, id, center, scale);
                if state.focus == id && (id != UNDO || active) {
                    self.brush.SetColor(&color(palette::accent(dark), 1.));
                    self.target.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: b,
                            radiusX: 6. * scale,
                            radiusY: 6. * scale,
                        },
                        self.brush,
                        1.5 * scale,
                        None,
                    );
                }
            }
        }
    }
}

fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 255) as f32 / 255.,
        g: ((rgb >> 8) & 255) as f32 / 255.,
        b: (rgb & 255) as f32 / 255.,
        a: alpha,
    }
}
