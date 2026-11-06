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
pub fn draw(
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    origin: PixelRect,
    editor: &Editor,
    layouts: &[Option<IDWriteTextLayout>],
    editing: Option<usize>,
) {
    let point = |p: PixelPoint| Vector2 {
        X: (i64::from(p.x) - i64::from(origin.left)) as f32,
        Y: (i64::from(p.y) - i64::from(origin.top)) as f32,
    };
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
                    let a = point(*start);
                    let b = point(*end);
                    target.DrawLine(a, b, brush, width, None);
                    for wing in arrow_head(*start, *end, width) {
                        target.DrawLine(b, point(wing), brush, width, None);
                    }
                }
                Shape::Pen(points) => {
                    for pair in points.windows(2) {
                        target.DrawLine(point(pair[0]), point(pair[1]), brush, width, None);
                    }
                }
            }
        }
        target.SetTextAntialiasMode(antialias);
    }
}
