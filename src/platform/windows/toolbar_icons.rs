//! Outline shapes measured against the capture design, in toolbar DIPs.
use super::{
    capture_toolbar::{BUTTON_HEIGHT, COPY, LONG_CAPTURE, Layout, MORE, MOSAIC, PICKER, SEQUENCE},
    ui_palette as palette,
};
use crate::core::{geometry::PixelRect, selection::PixelPoint};
use windows::{
    Win32::Graphics::Direct2D::{Common::*, *},
    core::Result,
};
use windows_numerics::{Matrix3x2, Vector2};

pub(super) struct ToolbarIcons {
    paths: [Option<ID2D1PathGeometry>; 10],
    picker: ID2D1PathGeometry,
    stroke: ID2D1StrokeStyle,
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
    (-6.5, -7.3),
    &[
        L((6.5, -7.3)),
        C((7.1, -7.3), (7.5, -6.9), (7.5, -6.3)),
        L((7.5, 6.3)),
        C((7.5, 6.9), (7.1, 7.3), (6.5, 7.3)),
        L((-6.5, 7.3)),
        C((-7.1, 7.3), (-7.5, 6.9), (-7.5, 6.3)),
        L((-7.5, -6.3)),
        C((-7.5, -6.9), (-7.1, -7.3), (-6.5, -7.3)),
    ],
    true,
)];
const ARROW: &[Figure] = &[
    Figure((-7.2, 7.2), &[L((7., -7.))], false),
    Figure((-1.8, -7.), &[L((7., -7.)), L((7., 1.8))], false),
];
const PEN: &[Figure] = &[
    Figure(
        (-7.9, 6.4),
        &[
            C((-8.4, 4.1), (-4.9, -1.1), (-1.2, -4.5)),
            C((2.5, -8.), (5.9, -9.2), (7., -7.6)),
            C((8.4, -5.6), (4.9, -1.1), (1.3, 2.3)),
            C((-2.6, 5.9), (-7.4, 8.7), (-7.9, 6.4)),
        ],
        true,
    ),
    Figure(
        (2.7, 6.4),
        &[
            C((4., 4.3), (6.1, 2.9), (6.7, 4.2)),
            C((6.9, 4.8), (5., 6.1), (5.7, 6.3)),
            C((6., 6.4), (6.8, 5.8), (7.6, 5.9)),
        ],
        false,
    ),
];
const TEXT: &[Figure] = &[
    Figure(
        (-7., -4.8),
        &[L((-7., -7.4)), L((7., -7.4)), L((7., -4.8))],
        false,
    ),
    Figure((0., -7.4), &[L((0., 7.4))], false),
    Figure((-2.7, 7.4), &[L((2.7, 7.4))], false),
];
const PIN: &[Figure] = &[
    Figure(
        (-0.8, -5.8),
        &[
            L((1.2, -7.8)),
            C((1.6, -8.2), (2.1, -8.2), (2.5, -7.8)),
            L((7.6, -2.7)),
            C((8., -2.3), (8., -1.8), (7.6, -1.4)),
            L((5.6, 0.6)),
            L((3.3, 0.1)),
            L((0.2, 3.2)),
            L((-0.5, 6.1)),
            L((-6.3, 0.3)),
            L((-3.4, -0.4)),
            L((-0.3, -3.5)),
        ],
        true,
    ),
    Figure((-3.4, 3.2), &[L((-7.7, 7.5))], false),
];
const SAVE: &[Figure] = &[
    Figure(
        (-6.5, -7.8),
        &[
            L((4.6, -7.8)),
            L((7.6, -4.8)),
            L((7.6, 6.5)),
            C((7.6, 7.2), (7.1, 7.7), (6.4, 7.7)),
            L((-6.4, 7.7)),
            C((-7.1, 7.7), (-7.6, 7.2), (-7.6, 6.5)),
            L((-7.6, -6.7)),
            C((-7.6, -7.4), (-7.1, -7.8), (-6.5, -7.8)),
        ],
        true,
    ),
    Figure(
        (-4.8, -7.8),
        &[L((-4.8, -2.2)), L((3.4, -2.2)), L((3.4, -7.8))],
        false,
    ),
    Figure(
        (-4.5, 7.7),
        &[L((-4.5, 2.2)), L((4.5, 2.2)), L((4.5, 7.7))],
        false,
    ),
];
const COPY_ICON: &[Figure] = &[
    Figure(
        (-4.1, -7.9),
        &[
            L((5.4, -7.9)),
            C((6.3, -7.9), (7., -7.2), (7., -6.3)),
            L((7., 3.3)),
            C((7., 4.2), (6.3, 4.9), (5.4, 4.9)),
            L((-4.1, 4.9)),
            C((-5., 4.9), (-5.7, 4.2), (-5.7, 3.3)),
            L((-5.7, -6.3)),
            C((-5.7, -7.2), (-5., -7.9), (-4.1, -7.9)),
        ],
        true,
    ),
    Figure(
        (-8.4, -4.4),
        &[
            L((-8.4, 6.4)),
            C((-8.4, 7.3), (-7.7, 8.), (-6.8, 8.)),
            L((3.4, 8.)),
        ],
        false,
    ),
];
const CLOSE: &[Figure] = &[
    Figure((-6.5, -6.5), &[L((6.5, 6.5))], false),
    Figure((6.5, -6.5), &[L((-6.5, 6.5))], false),
];
const EYEDROPPER: &[Figure] = &[
    Figure(
        (3.5, -8.),
        &[L((8., -3.5)), L((4.8, -0.3)), L((0.3, -4.8))],
        true,
    ),
    Figure(
        (1.5, -3.6),
        &[L((-6.5, 4.4)), L((-6.5, 7.)), L((-3.9, 7.)), L((3.6, -1.5))],
        false,
    ),
    Figure((-6.5, 7.), &[L((-8., 8.5))], false),
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
                    None,
                    None,
                    Some(path(PIN)?),
                    Some(path(SAVE)?),
                    Some(path(COPY_ICON)?),
                    Some(path(CLOSE)?),
                ],
                picker: path(EYEDROPPER)?,
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
                for offset in [-6., 0., 6.] {
                    target.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2 {
                                X: center.X + offset * scale,
                                ..center
                            },
                            radiusX: 1.5 * scale,
                            radiusY: 1.5 * scale,
                        },
                        brush,
                    );
                }
                return;
            }
            if index == MOSAIC {
                for row in 0..3 {
                    for column in 0..3 {
                        let left = center.X + (column as f32 * 5.6 - 7.9) * scale;
                        let top = center.Y + (row as f32 * 5.6 - 7.9) * scale;
                        let tile = D2D_RECT_F {
                            left,
                            top,
                            right: left + 4.6 * scale,
                            bottom: top + 4.6 * scale,
                        };
                        if (row + column) % 2 == 0 {
                            target.FillRectangle(&tile, brush);
                        } else {
                            target.DrawRectangle(&tile, brush, 0.8 * scale, None);
                        }
                    }
                }
                return;
            }
            if index == SEQUENCE {
                let radius = 8. * scale;
                target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: center,
                        radiusX: radius,
                        radiusY: radius,
                    },
                    brush,
                    1.4 * scale,
                    None,
                );
                let point = |x: f32, y: f32| Vector2 {
                    X: center.X + x * scale,
                    Y: center.Y + y * scale,
                };
                for (a, b) in [
                    (point(-2., -2.), point(0., -4.)),
                    (point(0., -4.), point(0., 4.)),
                    (point(-2., 4.), point(2., 4.)),
                ] {
                    target.DrawLine(a, b, brush, 1.4 * scale, &self.stroke);
                }
                return;
            }
            if index == LONG_CAPTURE {
                let point = |x: f32, y: f32| Vector2 {
                    X: center.X + x * scale,
                    Y: center.Y + y * scale,
                };
                for (a, b) in [
                    (point(-8., -8.), point(2., -8.)),
                    (point(-8., -8.), point(-8., 8.)),
                    (point(-8., 8.), point(2., 8.)),
                    (point(5., -7.), point(5., 7.)),
                    (point(2., -4.), point(5., -7.)),
                    (point(8., -4.), point(5., -7.)),
                    (point(2., 4.), point(5., 7.)),
                    (point(8., 4.), point(5., 7.)),
                ] {
                    target.DrawLine(a, b, brush, 1.4 * scale, &self.stroke);
                }
                return;
            }
            let path = if index == PICKER {
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
            target.DrawGeometry(path, brush, 1.4, &self.stroke);
            target.SetTransform(&prior);
        }
    }
}

#[derive(Default)]
pub(super) struct BarState {
    pub selected: usize,
    pub hover: Option<usize>,
    pub pressed: Option<usize>,
    pub focus: usize,
    pub busy: bool,
}

pub(super) struct BarPaint<'a> {
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
                let inset = (BUTTON_HEIGHT - 34.) * scale / 2.;
                let face = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        top: b.top + inset,
                        bottom: b.bottom - inset,
                        ..b
                    },
                    radiusX: 7. * scale,
                    radiusY: 7. * scale,
                };
                let active = !state.busy;
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
                if state.focus == id {
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
