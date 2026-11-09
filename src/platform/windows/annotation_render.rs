//! Shared annotation geometry for preview and clean output. No selection or toolbar decoration.
use crate::core::{
    annotation::{Editor, Shape, arrow_head},
    geometry::PixelRect,
    selection::PixelPoint,
};
use windows::{
    Win32::Graphics::{
        Direct2D::{Common::*, *},
        DirectWrite::*,
    },
    core::{Result, w},
};
use windows_numerics::Vector2;
pub fn text_layout(
    factory: &IDWriteFactory,
    content: &str,
    size: f32,
    width: f32,
) -> Result<IDWriteTextLayout> {
    // SAFETY: live thread-local factory and call-scoped UTF-16 input; DirectWrite owns copied layout text.
    unsafe {
        let format = factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("zh-CN"),
        )?;
        let wide: Vec<u16> = content.encode_utf16().collect();
        factory.CreateTextLayout(&wide, &format, width, 10_000_000.)
    }
}
pub fn layouts(
    factory: &IDWriteFactory,
    editor: &Editor,
) -> Result<Vec<Option<IDWriteTextLayout>>> {
    editor
        .objects()
        .iter()
        .map(|o| match &o.shape {
            Shape::Text {
                content,
                size,
                wrap_width,
                ..
            } => text_layout(factory, content, *size, *wrap_width).map(Some),
            _ => Ok(None),
        })
        .collect()
}
pub struct Geometry {
    paths: Vec<Option<ID2D1PathGeometry>>,
    round: ID2D1StrokeStyle,
}

fn local_point(origin: PixelRect, p: PixelPoint) -> Vector2 {
    Vector2 {
        X: (i64::from(p.x) - i64::from(origin.left)) as f32,
        Y: (i64::from(p.y) - i64::from(origin.top)) as f32,
    }
}

pub fn geometry(factory: &ID2D1Factory, origin: PixelRect, editor: &Editor) -> Result<Geometry> {
    let point = |p| local_point(origin, p);
    // SAFETY: caller owns the factory on this thread. Geometry sinks are closed before the paths are drawn.
    unsafe {
        let round = factory.CreateStrokeStyle(
            &D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 1.,
                ..Default::default()
            },
            None,
        )?;
        let paths = editor
            .objects()
            .iter()
            .map(|object| {
                let (points, filled) = match &object.shape {
                    Shape::Arrow(a, b) if a != b => {
                        let [left, right] = arrow_head(*a, *b, object.style.width);
                        (vec![*b, left, right], true)
                    }
                    Shape::Pen(points) if points.len() > 1 => (points.clone(), false),
                    _ => return Ok(None),
                };
                let path = factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                sink.BeginFigure(
                    point(points[0]),
                    if filled {
                        D2D1_FIGURE_BEGIN_FILLED
                    } else {
                        D2D1_FIGURE_BEGIN_HOLLOW
                    },
                );
                let lines: Vec<_> = points[1..].iter().copied().map(point).collect();
                sink.AddLines(&lines);
                sink.EndFigure(if filled {
                    D2D1_FIGURE_END_CLOSED
                } else {
                    D2D1_FIGURE_END_OPEN
                });
                sink.Close()?;
                Ok(Some(path))
            })
            .collect::<Result<_>>()?;
        Ok(Geometry { paths, round })
    }
}

impl Geometry {
    pub fn stroke(
        &self,
        target: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        origin: PixelRect,
        index: usize,
        points: &[PixelPoint],
        width: f32,
    ) {
        // SAFETY: paths, style and brush are live on the drawing thread, with an active draw pass.
        unsafe {
            if let Some(path) = &self.paths[index] {
                target.DrawGeometry(path, brush, width, &self.round);
            } else if let Some(p) = points.first() {
                target.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: local_point(origin, *p),
                        radiusX: width / 2.,
                        radiusY: width / 2.,
                    },
                    brush,
                );
            }
        }
    }
}

pub fn draw(
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    origin: PixelRect,
    editor: &Editor,
    prepared: (&[Option<IDWriteTextLayout>], &Geometry),
    editing: Option<usize>,
) {
    let (layouts, geometry) = prepared;
    let point = |p| local_point(origin, p);
    let local = |r: PixelRect| D2D_RECT_F {
        left: (i64::from(r.left) - i64::from(origin.left)) as f32,
        top: (i64::from(r.top) - i64::from(origin.top)) as f32,
        right: (i64::from(r.right) - i64::from(origin.left)) as f32,
        bottom: (i64::from(r.bottom) - i64::from(origin.top)) as f32,
    };
    // SAFETY: caller retains target, brush and layouts on the owning drawing thread; all calls stay between BeginDraw/EndDraw.
    unsafe {
        let antialias = target.GetTextAntialiasMode();
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        for (index, object) in editor.objects().iter().enumerate() {
            if editing == Some(index) {
                continue;
            }
            let c = object.style.color;
            brush.SetColor(&D2D1_COLOR_F {
                r: c[0] as f32 / 255.,
                g: c[1] as f32 / 255.,
                b: c[2] as f32 / 255.,
                a: 1.,
            });
            let width = object.style.width;
            match &object.shape {
                Shape::Text { bounds, .. } => {
                    if let Some(layout) = &layouts[index] {
                        target.DrawTextLayout(
                            point(PixelPoint {
                                x: bounds.left,
                                y: bounds.top,
                            }),
                            layout,
                            brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    }
                }
                Shape::Rectangle(r) => target.DrawRectangle(&local(*r), brush, width, None),
                Shape::Arrow(start, end) => {
                    if let Some(head) = &geometry.paths[index] {
                        let [left, right] = arrow_head(*start, *end, width).map(point);
                        let tip = point(*end);
                        // Stop the shaft inside the filled head so its flat cap cannot protrude beyond the tip.
                        let stem = Vector2 {
                            X: (tip.X * 2. + left.X + right.X) / 4.,
                            Y: (tip.Y * 2. + left.Y + right.Y) / 4.,
                        };
                        target.DrawLine(point(*start), stem, brush, width, None);
                        target.FillGeometry(head, brush, None);
                    }
                }
                Shape::Pen(points) => geometry.stroke(target, brush, origin, index, points, width),
            }
        }
        target.SetTextAntialiasMode(antialias);
    }
}
