//! Outline shapes measured against the capture design, in toolbar DIPs.
use windows::{
    Win32::Graphics::Direct2D::{Common::*, *},
    core::Result,
};
use windows_numerics::{Matrix3x2, Vector2};

pub(super) struct ToolbarIcons {
    paths: [ID2D1PathGeometry; 11],
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

const POINTER: &[Figure] = &[Figure(
    (-5.5, -8.2),
    &[
        L((-5.5, 7.1)),
        L((-2., 3.5)),
        L((0.5, 8.1)),
        L((2.8, 6.8)),
        L((0.3, 2.)),
        L((6.1, 1.9)),
    ],
    true,
)];
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
const UNDO: &[Figure] = &[
    Figure(
        (-7.5, -3.8),
        &[
            L((0.8, -3.8)),
            C((8.3, -3.8), (9.3, 7.3), (0.6, 7.3)),
            L((-2.2, 7.3)),
        ],
        false,
    ),
    Figure((-3.4, -7.9), &[L((-7.5, -3.8)), L((-3.4, 0.3))], false),
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
const COPY: &[Figure] = &[
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
            let path = |figures: &[Figure], mirror: bool| -> Result<ID2D1PathGeometry> {
                let geometry = factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                let point = |(x, y): Point| Vector2 {
                    X: if mirror { -x } else { x },
                    Y: y,
                };
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
                    path(POINTER, false)?,
                    path(RECTANGLE, false)?,
                    path(ARROW, false)?,
                    path(PEN, false)?,
                    path(TEXT, false)?,
                    path(UNDO, false)?,
                    path(UNDO, true)?,
                    path(PIN, false)?,
                    path(SAVE, false)?,
                    path(COPY, false)?,
                    path(CLOSE, false)?,
                ],
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
            target.DrawGeometry(&self.paths[index], brush, 1.4, &self.stroke);
            target.SetTransform(&prior);
        }
    }
}
