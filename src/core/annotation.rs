//! Editable annotations in global physical pixels; no native handles or image copies.
use super::{
    config::{
        CaptureParameter, DEFAULT_MOSAIC_SIZE, DEFAULT_TEXT_SIZE, MAX_STROKE_WIDTH,
        MIN_STROKE_WIDTH,
    },
    geometry::PixelRect,
    selection::{PixelPoint, Selection, contains},
};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rectangle,
    Arrow,
    Pen,
    Text,
    Mosaic,
}
impl Tool {
    pub const DRAWING: [Self; 5] = [
        Self::Rectangle,
        Self::Arrow,
        Self::Pen,
        Self::Text,
        Self::Mosaic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "选择",
            Self::Rectangle => "矩形",
            Self::Arrow => "箭头",
            Self::Pen => "画笔",
            Self::Text => "文字",
            Self::Mosaic => "马赛克",
        }
    }

    pub fn parameter(self) -> Option<CaptureParameter> {
        match self {
            Self::Text => Some(CaptureParameter::TextSize),
            Self::Mosaic => Some(CaptureParameter::MosaicSize),
            _ => self.stroke_index().map(CaptureParameter::Stroke),
        }
    }

    pub fn has_color(self) -> bool {
        self == Self::Text || self.stroke_index().is_some()
    }

    pub fn stroke_index(self) -> Option<usize> {
        match self {
            Self::Rectangle => Some(0),
            Self::Arrow => Some(1),
            Self::Pen => Some(2),
            Self::Select | Self::Text | Self::Mosaic => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub color: [u8; 3],
    pub width: f32,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            color: [255, 59, 64],
            width: 3.,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rectangle(PixelRect),
    Mosaic {
        bounds: PixelRect,
        size: u16,
    },
    Arrow(PixelPoint, PixelPoint),
    Pen(Vec<PixelPoint>),
    Text {
        bounds: PixelRect,
        content: String,
        size: f32,
        wrap_width: f32,
        angle: f64,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub shape: Shape,
    pub style: Style,
}
impl Object {
    pub fn matches_tool(&self, tool: Tool) -> bool {
        matches!(
            (&self.shape, tool),
            (_, Tool::Select)
                | (Shape::Rectangle(_), Tool::Rectangle)
                | (Shape::Arrow(..), Tool::Arrow)
                | (Shape::Pen(_), Tool::Pen)
                | (Shape::Text { .. }, Tool::Text)
                | (Shape::Mosaic { .. }, Tool::Mosaic)
        )
    }
    pub fn bounds(&self) -> PixelRect {
        match &self.shape {
            Shape::Text { bounds, .. } | Shape::Mosaic { bounds, .. } => *bounds,
            Shape::Rectangle(r) => *r,
            Shape::Arrow(a, b) => {
                let mut bounds = rect(*a, *b);
                for p in arrow_head(*a, *b, self.style.width) {
                    bounds.left = bounds.left.min(p.x);
                    bounds.top = bounds.top.min(p.y);
                    bounds.right = bounds.right.max(p.x);
                    bounds.bottom = bounds.bottom.max(p.y);
                }
                bounds
            }
            Shape::Pen(points) => points.iter().fold(
                rect(
                    points.first().copied().unwrap_or(PixelPoint { x: 0, y: 0 }),
                    points.first().copied().unwrap_or(PixelPoint { x: 0, y: 0 }),
                ),
                |r, p| PixelRect {
                    left: r.left.min(p.x),
                    top: r.top.min(p.y),
                    right: r.right.max(p.x),
                    bottom: r.bottom.max(p.y),
                },
            ),
        }
    }
    pub fn handles(&self) -> Vec<PixelPoint> {
        match &self.shape {
            Shape::Rectangle(r) | Shape::Mosaic { bounds: r, .. } => {
                Selection::handles(*r).to_vec()
            }
            Shape::Arrow(a, b) => vec![*a, *b],
            Shape::Pen(_) | Shape::Text { .. } => Vec::new(),
        }
    }
    pub fn rotation_handle(&self, scale: f32) -> Option<PixelPoint> {
        let Shape::Text { bounds, angle, .. } = self.shape else {
            return None;
        };
        Some(text_rotation_handle(bounds, angle, scale))
    }
    fn hit(&self, p: PixelPoint, tolerance: f64) -> bool {
        let t = tolerance + f64::from(self.style.width) / 2.;
        match &self.shape {
            Shape::Mosaic { bounds, .. } => contains(*bounds, p),
            Shape::Text { bounds, angle, .. } => {
                let (x, y) = rotate_point(*bounds, -*angle, f64::from(p.x), f64::from(p.y));
                x >= f64::from(bounds.left)
                    && x < f64::from(bounds.right)
                    && y >= f64::from(bounds.top)
                    && y < f64::from(bounds.bottom)
            }
            Shape::Rectangle(r) => {
                let h = Selection::handles(*r);
                [(0, 2), (2, 4), (4, 6), (6, 0)]
                    .iter()
                    .any(|(a, b)| distance(p, h[*a], h[*b]) <= t)
            }
            Shape::Arrow(a, b) => {
                let [left, right] = arrow_head(*a, *b, self.style.width);
                let edges = [(*b, left), (left, right), (right, *b)];
                let cross = |(a, b): (PixelPoint, PixelPoint)| {
                    (f64::from(b.x) - f64::from(a.x)) * (f64::from(p.y) - f64::from(a.y))
                        - (f64::from(b.y) - f64::from(a.y)) * (f64::from(p.x) - f64::from(a.x))
                };
                let signs = edges.map(cross);
                distance(p, *a, *b) <= t
                    || edges.iter().any(|(a, b)| distance(p, *a, *b) <= t)
                    || (left != right
                        && (signs.iter().all(|s| *s >= 0.) || signs.iter().all(|s| *s <= 0.)))
            }
            Shape::Pen(points) => {
                points
                    .windows(2)
                    .any(|pair| distance(p, pair[0], pair[1]) <= t)
                    || points.first().is_some_and(|a| distance(p, *a, *a) <= t)
            }
        }
    }
}
/// Arrow wings use the same physical geometry for rendering, hit testing and bounds.
pub fn arrow_head(a: PixelPoint, b: PixelPoint, width: f32) -> [PixelPoint; 2] {
    let dx = f64::from(b.x) - f64::from(a.x);
    let dy = f64::from(b.y) - f64::from(a.y);
    let length = dx.hypot(dy);
    if length == 0. {
        return [b; 2];
    }
    let head = (f64::from(width) * 4.).max(10.).min(length);
    let ux = dx / length;
    let uy = dy / length;
    [-1., 1.].map(|side| PixelPoint {
        x: (f64::from(b.x) - head * ux + side * head * 0.5 * uy).round() as i32,
        y: (f64::from(b.y) - head * uy - side * head * 0.5 * ux).round() as i32,
    })
}
pub fn text_center(bounds: PixelRect) -> (f64, f64) {
    (
        (f64::from(bounds.left) + f64::from(bounds.right)) / 2.,
        (f64::from(bounds.top) + f64::from(bounds.bottom)) / 2.,
    )
}

pub fn rotate_point(bounds: PixelRect, angle: f64, x: f64, y: f64) -> (f64, f64) {
    let (cx, cy) = text_center(bounds);
    let (sin, cos) = angle.sin_cos();
    let (x, y) = (x - cx, y - cy);
    (cx + x * cos - y * sin, cy + x * sin + y * cos)
}

pub fn text_frame(bounds: PixelRect, scale: f32) -> PixelRect {
    pad_text_bounds(bounds, (5. * scale).ceil() as i32)
}

pub fn text_frame_hit_bounds(bounds: PixelRect, scale: f32) -> PixelRect {
    pad_text_bounds(text_frame(bounds, scale), (7. * scale).ceil() as i32)
}

fn pad_text_bounds(bounds: PixelRect, padding: i32) -> PixelRect {
    PixelRect {
        left: bounds.left.saturating_sub(padding),
        top: bounds.top.saturating_sub(padding),
        right: bounds.right.saturating_add(padding),
        bottom: bounds.bottom.saturating_add(padding),
    }
}

pub fn text_rotation_handle(bounds: PixelRect, angle: f64, scale: f32) -> PixelPoint {
    let (cx, _) = text_center(bounds);
    let (x, y) = rotate_point(
        bounds,
        angle,
        cx,
        f64::from(text_frame(bounds, scale).top) - 16. * f64::from(scale),
    );
    PixelPoint {
        x: x.round() as i32,
        y: y.round() as i32,
    }
}

pub fn hits_text_rotation(p: PixelPoint, handle: PixelPoint, scale: f32) -> bool {
    distance(p, handle, handle) <= 9. * f64::from(scale)
}

fn rect(a: PixelPoint, b: PixelPoint) -> PixelRect {
    PixelRect {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    }
}
fn offset(value: i32, start: i32, end: i32) -> i32 {
    (i64::from(value) + i64::from(end) - i64::from(start))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
fn distance(p: PixelPoint, a: PixelPoint, b: PixelPoint) -> f64 {
    let (x, y) = (
        f64::from(p.x) - f64::from(a.x),
        f64::from(p.y) - f64::from(a.y),
    );
    let (dx, dy) = (
        f64::from(b.x) - f64::from(a.x),
        f64::from(b.y) - f64::from(a.y),
    );
    let length = dx * dx + dy * dy;
    let t = if length == 0. {
        0.
    } else {
        ((x * dx + y * dy) / length).clamp(0., 1.)
    };
    (x - t * dx).hypot(y - t * dy)
}
#[derive(Clone)]
struct Change {
    index: usize,
    before: Option<Object>,
    after: Option<Object>,
}
enum ParameterBefore {
    MosaicSize {
        index: usize,
        size: u16,
    },
    StrokeWidth {
        index: usize,
        width: f32,
    },
    TextSize {
        index: usize,
        bounds: PixelRect,
        size: f32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectHit {
    Move(usize),
    Handle(usize, usize),
    Rotate(usize),
    Text(usize),
}

enum DragKind {
    Create,
    Move,
    Handle(usize),
    Rotate,
}
struct Drag {
    index: usize,
    start: PixelPoint,
    before: Option<Object>,
    kind: DragKind,
}
pub struct Editor {
    pub tool: Tool,
    pub styles: [Style; 4],
    objects: Vec<Object>,
    pub selected: Option<usize>,
    pub text_size: u16,
    pub mosaic_size: u16,
    drag: Option<Drag>,
    parameter_before: Option<ParameterBefore>,
    undo: VecDeque<Change>,
    redo: Vec<Change>,
    warning: Option<&'static str>,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            styles: [Style::default(); 4],
            objects: Vec::new(),
            selected: None,
            text_size: DEFAULT_TEXT_SIZE,
            mosaic_size: DEFAULT_MOSAIC_SIZE,
            drag: None,
            parameter_before: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            warning: None,
        }
    }
}
impl Editor {
    pub fn text_at(&self, p: PixelPoint) -> Option<usize> {
        self.front_to_back()
            .find(|(_, object)| object.hit(p, 0.))
            .map(|(index, _)| index)
            .filter(|i| matches!(self.objects[*i].shape, Shape::Text { .. }))
    }
    pub fn commit_text(
        &mut self,
        index: Option<usize>,
        object: Object,
    ) -> Result<(), &'static str> {
        self.finish_parameter_adjustment();
        let Shape::Text {
            bounds,
            ref content,
            size,
            wrap_width,
            angle,
        } = object.shape
        else {
            return Err("无效文字对象");
        };
        if bounds.size().is_none()
            || !angle.is_finite()
            || !size.is_finite()
            || size <= 0.
            || !wrap_width.is_finite()
            || wrap_width <= 0.
            || content.replace('\n', "\r\n").encode_utf16().count() > 16384
        {
            return Err("文字尺寸或长度无效");
        }
        if self.active() {
            return Err("请先结束当前拖动");
        }
        if let Some(index) = index {
            if !self
                .objects
                .get(index)
                .is_some_and(|o| matches!(o.shape, Shape::Text { .. }))
            {
                return Err("文字对象已失效");
            }
            let before = self.objects[index].clone();
            let after = if content.is_empty() {
                self.objects.remove(index);
                self.selected = None;
                None
            } else {
                self.objects[index] = object.clone();
                self.selected = Some(index);
                Some(object)
            };
            self.record(Change {
                index,
                before: Some(before),
                after,
            });
        } else if !content.is_empty() {
            if self.objects.len() >= 1024 {
                return Err("标注数量已达到 1024 个上限");
            }
            self.objects
                .try_reserve(1)
                .map_err(|_| "没有足够内存保存文字")?;
            let index = self.objects.len();
            self.objects.push(object.clone());
            self.selected = Some(index);
            self.record(Change {
                index,
                before: None,
                after: Some(object),
            });
        }
        Ok(())
    }
    pub fn warn(&mut self, message: &'static str) {
        self.warning = Some(message);
    }
    pub fn take_warning(&mut self) -> Option<&'static str> {
        self.warning.take()
    }
    pub fn snapshot(&self) -> Self {
        Self {
            objects: self.objects.clone(),
            tool: self.tool,
            styles: self.styles,
            text_size: self.text_size,
            mosaic_size: self.mosaic_size,
            ..Default::default()
        }
    }
    pub fn objects(&self) -> &[Object] {
        &self.objects
    }
    pub fn mosaics(&self) -> impl Iterator<Item = (PixelRect, u16)> + '_ {
        self.objects.iter().filter_map(|object| match object.shape {
            Shape::Mosaic { bounds, size } => Some((bounds, size)),
            _ => None,
        })
    }
    fn front_to_back(&self) -> impl Iterator<Item = (usize, &Object)> {
        let objects = self.objects.iter().enumerate().rev();
        let is_mosaic = |entry: &(usize, &Object)| matches!(entry.1.shape, Shape::Mosaic { .. });
        objects
            .clone()
            .filter(move |entry| !is_mosaic(entry))
            .chain(objects.filter(is_mosaic))
    }
    pub fn active(&self) -> bool {
        self.drag.is_some()
    }
    pub fn can_undo(&self) -> bool {
        (!self.undo.is_empty() || self.parameter_before.is_some()) && !self.active()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty() && !self.active() && self.parameter_before.is_none()
    }
    pub fn style(&self) -> Style {
        if let Some(index) = self.selected_stroke() {
            return self.objects[index].style;
        }
        self.default_style()
    }
    fn default_style(&self) -> Style {
        self.styles[self.tool.stroke_index().unwrap_or(3)]
    }
    pub fn current_mosaic_size(&self) -> u16 {
        match self.selected.and_then(|index| self.objects.get(index)) {
            Some(Object {
                shape: Shape::Mosaic { size, .. },
                ..
            }) => *size,
            _ => self.mosaic_size,
        }
    }
    pub fn set_mosaic_size(&mut self, value: u16) {
        self.mosaic_size = value;
        let Some(index) = self.selected else { return };
        if let Shape::Mosaic { size, .. } = &mut self.objects[index].shape
            && *size != value
        {
            if self.drag.is_none() {
                self.parameter_before
                    .get_or_insert(ParameterBefore::MosaicSize { index, size: *size });
            }
            *size = value;
        }
    }
    fn selected_stroke(&self) -> Option<usize> {
        self.tool.stroke_index()?;
        let index = self.selected?;
        self.objects[index].matches_tool(self.tool).then_some(index)
    }
    pub fn set_stroke_width(&mut self, width: u8) {
        let Some(tool) = self.tool.stroke_index() else {
            return;
        };
        if !(MIN_STROKE_WIDTH..=MAX_STROKE_WIDTH).contains(&width) {
            return;
        }
        let width = f32::from(width);
        self.styles[tool].width = width;
        if let Some(index) = self.selected_stroke() {
            let previous = self.objects[index].style.width;
            if previous != width {
                if !self.active() {
                    self.parameter_before
                        .get_or_insert(ParameterBefore::StrokeWidth {
                            index,
                            width: previous,
                        });
                }
                self.objects[index].style.width = width;
            }
        }
    }
    pub fn text_resize_anchor(&self, index: usize, current: PixelRect) -> PixelRect {
        match self.parameter_before {
            Some(ParameterBefore::TextSize {
                index: adjusted,
                bounds,
                ..
            }) if adjusted == index => bounds,
            _ => current,
        }
    }
    pub fn resize_text(&mut self, index: usize, next: PixelRect, next_size: f32) {
        let Shape::Text { bounds, size, .. } = &mut self.objects[index].shape else {
            return;
        };
        if *bounds != next || *size != next_size {
            if self.drag.is_none() {
                self.parameter_before
                    .get_or_insert(ParameterBefore::TextSize {
                        index,
                        bounds: *bounds,
                        size: *size,
                    });
            }
            *bounds = next;
            *size = next_size;
        }
    }
    pub fn finish_parameter_adjustment(&mut self) {
        let Some(adjustment) = self.parameter_before.take() else {
            return;
        };
        let index = match adjustment {
            ParameterBefore::StrokeWidth { index, .. }
            | ParameterBefore::TextSize { index, .. }
            | ParameterBefore::MosaicSize { index, .. } => index,
        };
        let after = self.objects[index].clone();
        let mut before = after.clone();
        match adjustment {
            ParameterBefore::StrokeWidth { width, .. } => before.style.width = width,
            ParameterBefore::MosaicSize { size, .. } => {
                if let Shape::Mosaic { size: old_size, .. } = &mut before.shape {
                    *old_size = size;
                }
            }
            ParameterBefore::TextSize { bounds, size, .. } => {
                if let Shape::Text {
                    bounds: old_bounds,
                    size: old_size,
                    ..
                } = &mut before.shape
                {
                    *old_bounds = bounds;
                    *old_size = size;
                }
            }
        }
        self.record(Change {
            index,
            before: Some(before),
            after: Some(after),
        });
    }
    pub fn set_stroke_color(&mut self, color: [u8; 3]) {
        self.finish_parameter_adjustment();
        let Some(tool) = self.tool.stroke_index() else {
            return;
        };
        self.styles[tool].color = color;
        if let Some(index) = self.selected_stroke() {
            let before = self.objects[index].clone();
            self.objects[index].style.color = color;
            let after = self.objects[index].clone();
            self.record(Change {
                index,
                before: Some(before),
                after: Some(after),
            });
        }
    }
    pub fn hit_at(
        &self,
        p: PixelPoint,
        tolerance: i32,
        scale: f32,
        inside_selection: bool,
    ) -> Option<ObjectHit> {
        let tolerance = f64::from(tolerance);
        let selected = self
            .selected
            .filter(|i| self.objects[*i].matches_tool(self.tool));
        if let Some(index) = selected {
            let object = &self.objects[index];
            if object
                .rotation_handle(scale)
                .is_some_and(|handle| hits_text_rotation(p, handle, scale))
            {
                return Some(ObjectHit::Rotate(index));
            }
            if inside_selection {
                if let Some(handle) = object
                    .handles()
                    .into_iter()
                    .enumerate()
                    .map(|(i, h)| (i, distance(p, h, h)))
                    .filter(|(_, d)| *d <= tolerance)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
                {
                    return Some(ObjectHit::Handle(index, handle));
                }
            }
        }
        if !inside_selection {
            return None;
        }
        self.front_to_back().find_map(|(index, object)| {
            if !object.matches_tool(self.tool) {
                return None;
            }
            if self.tool == Tool::Text {
                let Shape::Text { bounds, angle, .. } = object.shape else {
                    return None;
                };
                let (x, y) = rotate_point(bounds, -angle, f64::from(p.x), f64::from(p.y));
                let inset = (x - f64::from(bounds.left))
                    .min(f64::from(bounds.right) - x)
                    .min(y - f64::from(bounds.top))
                    .min(f64::from(bounds.bottom) - y);
                if inset > 0.5 {
                    return Some(ObjectHit::Text(index));
                }
                let frame = if selected == Some(index) {
                    text_frame_hit_bounds(bounds, scale)
                } else {
                    bounds
                };
                if x >= f64::from(frame.left)
                    && x <= f64::from(frame.right)
                    && y >= f64::from(frame.top)
                    && y <= f64::from(frame.bottom)
                {
                    return Some(ObjectHit::Move(index));
                }
            } else if object.hit(p, tolerance) {
                if self.tool == Tool::Arrow {
                    if let Shape::Arrow(a, b) = object.shape {
                        let from = distance(p, a, a);
                        let to = distance(p, b, b);
                        if from.min(to) <= tolerance {
                            return Some(ObjectHit::Handle(index, usize::from(to < from)));
                        }
                    }
                }
                return Some(ObjectHit::Move(index));
            }
            None
        })
    }
    #[cfg(any(test, feature = "diagnostics"))]
    pub fn begin(
        &mut self,
        p: PixelPoint,
        tolerance: i32,
        scale: f32,
    ) -> Result<bool, &'static str> {
        let hit = self.hit_at(p, tolerance, scale, true);
        self.begin_at(p, hit)
    }
    pub fn begin_at(
        &mut self,
        p: PixelPoint,
        hit: Option<ObjectHit>,
    ) -> Result<bool, &'static str> {
        self.finish_parameter_adjustment();
        self.cancel();
        self.warning = None;
        if let Some(hit) = hit {
            let (index, kind) = match hit {
                ObjectHit::Move(index) => (index, DragKind::Move),
                ObjectHit::Handle(index, handle) => (index, DragKind::Handle(handle)),
                ObjectHit::Rotate(index) => (index, DragKind::Rotate),
                ObjectHit::Text(_) => return Ok(false),
            };
            let before = self.objects.get(index).ok_or("标注对象已失效")?.clone();
            self.selected = Some(index);
            self.drag = Some(Drag {
                index,
                start: p,
                before: Some(before),
                kind,
            });
            return Ok(true);
        }
        if matches!(self.tool, Tool::Select | Tool::Text) {
            self.selected = None;
            return Ok(false);
        }
        if self.objects.len() >= 1024 {
            return Err("标注数量已达到 1024 个上限，请删除部分对象后重试");
        }
        self.objects
            .try_reserve(1)
            .map_err(|_| "没有足够内存创建标注")?;
        let shape = match self.tool {
            Tool::Rectangle => Shape::Rectangle(rect(p, p)),
            Tool::Arrow => Shape::Arrow(p, p),
            Tool::Pen => Shape::Pen(vec![p]),
            Tool::Mosaic => Shape::Mosaic {
                bounds: rect(p, p),
                size: self.mosaic_size,
            },
            Tool::Select | Tool::Text => return Ok(false),
        };
        let style = self.default_style();
        let index = self.objects.len();
        self.objects.push(Object { shape, style });
        self.selected = Some(index);
        self.drag = Some(Drag {
            index,
            start: p,
            before: None,
            kind: DragKind::Create,
        });
        Ok(true)
    }
    pub fn update(&mut self, p: PixelPoint) {
        let Some(drag) = &self.drag else {
            return;
        };
        let object = &mut self.objects[drag.index];
        match &drag.kind {
            DragKind::Create => match &mut object.shape {
                Shape::Text { .. } => {}
                Shape::Rectangle(r) | Shape::Mosaic { bounds: r, .. } => *r = rect(drag.start, p),
                Shape::Arrow(_, b) => *b = p,
                Shape::Pen(points) => {
                    if points.len() >= 16384 && points.last() != Some(&p) {
                        self.warning = Some(
                            "本笔已达到 16384 个采样点上限，后续轨迹未记录；可撤销后分段绘制。",
                        );
                    }
                    if points.len() < 16384 && points.last() != Some(&p) {
                        points.push(p);
                    }
                }
            },
            DragKind::Move => {
                if let Some(before) = &drag.before {
                    let bounds = before.bounds();
                    let dx = (i64::from(p.x) - i64::from(drag.start.x)).clamp(
                        i64::from(i32::MIN) - i64::from(bounds.left),
                        i64::from(i32::MAX) - i64::from(bounds.right),
                    );
                    let dy = (i64::from(p.y) - i64::from(drag.start.y)).clamp(
                        i64::from(i32::MIN) - i64::from(bounds.top),
                        i64::from(i32::MAX) - i64::from(bounds.bottom),
                    );
                    let shift = |v: PixelPoint| PixelPoint {
                        x: (i64::from(v.x) + dx) as i32,
                        y: (i64::from(v.y) + dy) as i32,
                    };
                    let shift_rect = |r: PixelRect| {
                        rect(
                            shift(PixelPoint {
                                x: r.left,
                                y: r.top,
                            }),
                            shift(PixelPoint {
                                x: r.right,
                                y: r.bottom,
                            }),
                        )
                    };
                    match (&mut object.shape, &before.shape) {
                        (Shape::Rectangle(r), Shape::Rectangle(original))
                        | (
                            Shape::Mosaic { bounds: r, .. },
                            Shape::Mosaic {
                                bounds: original, ..
                            },
                        )
                        | (
                            Shape::Text { bounds: r, .. },
                            Shape::Text {
                                bounds: original, ..
                            },
                        ) => {
                            *r = shift_rect(*original);
                        }
                        (Shape::Arrow(a, b), Shape::Arrow(from, to)) => {
                            (*a, *b) = (shift(*from), shift(*to));
                        }
                        (Shape::Pen(points), Shape::Pen(original)) => {
                            for (point, &previous) in points.iter_mut().zip(original) {
                                *point = shift(previous);
                            }
                        }
                        _ => {}
                    }
                }
            }
            DragKind::Rotate => {
                if let (
                    Some(Object {
                        shape:
                            Shape::Text {
                                bounds,
                                angle: original,
                                ..
                            },
                        ..
                    }),
                    Shape::Text { angle, .. },
                ) = (&drag.before, &mut object.shape)
                {
                    let (x, y) = text_center(*bounds);
                    let vector = |p: PixelPoint| (f64::from(p.x) - x, f64::from(p.y) - y);
                    let (a, b) = vector(drag.start);
                    let (c, d) = vector(p);
                    if p == drag.start {
                        *angle = *original;
                    } else if c.hypot(d) >= 1. {
                        *angle = (original + d.atan2(c) - b.atan2(a) + std::f64::consts::PI)
                            .rem_euclid(std::f64::consts::TAU)
                            - std::f64::consts::PI;
                    }
                }
            }
            DragKind::Handle(index) => {
                if let Some(before) = &drag.before {
                    let x = |value| offset(value, drag.start.x, p.x);
                    let y = |value| offset(value, drag.start.y, p.y);
                    match &before.shape {
                        Shape::Rectangle(original)
                        | Shape::Mosaic {
                            bounds: original, ..
                        } => {
                            let mut r = *original;
                            if [0, 6, 7].contains(index) {
                                r.left = x(r.left);
                            }
                            if [2, 3, 4].contains(index) {
                                r.right = x(r.right);
                            }
                            if [0, 1, 2].contains(index) {
                                r.top = y(r.top);
                            }
                            if [4, 5, 6].contains(index) {
                                r.bottom = y(r.bottom);
                            }
                            let next = rect(
                                PixelPoint {
                                    x: r.left,
                                    y: r.top,
                                },
                                PixelPoint {
                                    x: r.right,
                                    y: r.bottom,
                                },
                            );
                            if let Shape::Rectangle(bounds) | Shape::Mosaic { bounds, .. } =
                                &mut object.shape
                            {
                                *bounds = next;
                            }
                        }
                        Shape::Arrow(a, b) => {
                            let shift = |end: PixelPoint| PixelPoint {
                                x: x(end.x),
                                y: y(end.y),
                            };
                            object.shape = if *index == 0 {
                                Shape::Arrow(shift(*a), *b)
                            } else {
                                Shape::Arrow(*a, shift(*b))
                            }
                        }
                        Shape::Pen(_) | Shape::Text { .. } => {}
                    }
                }
            }
        }
    }
    pub fn finish(&mut self, p: PixelPoint) {
        self.update(p);
        let Some(drag) = self.drag.take() else {
            return;
        };
        let object = &self.objects[drag.index];
        let valid = match &object.shape {
            Shape::Text { bounds, .. } | Shape::Mosaic { bounds, .. } => bounds.size().is_some(),
            Shape::Rectangle(r) => r.size().is_some(),
            Shape::Arrow(a, b) => a != b,
            Shape::Pen(points) => !points.is_empty(),
        };
        if !valid {
            if let Some(before) = drag.before {
                self.objects[drag.index] = before;
            } else {
                self.objects.remove(drag.index);
                self.selected = None;
            }
            return;
        }
        if drag.before.as_ref() == Some(object) {
            return;
        }
        self.record(Change {
            index: drag.index,
            before: drag.before,
            after: Some(self.objects[drag.index].clone()),
        });
    }
    pub fn cancel(&mut self) -> bool {
        if let Some(drag) = self.drag.take() {
            if let Some(before) = drag.before {
                self.objects[drag.index] = before;
            } else {
                self.objects.remove(drag.index);
                self.selected = None;
            }
            true
        } else {
            false
        }
    }
    fn record(&mut self, change: Change) {
        if change.before == change.after {
            return;
        }
        self.redo.clear();
        if self.undo.len() == 100 {
            self.undo.pop_front();
        }
        self.undo.push_back(change);
    }
    fn apply(&mut self, index: usize, from: &Option<Object>, to: &Option<Object>) {
        match (from, to) {
            (None, Some(object)) => self.objects.insert(index, object.clone()),
            (Some(_), None) => {
                self.objects.remove(index);
            }
            (Some(_), Some(object)) => self.objects[index] = object.clone(),
            _ => {}
        }
        self.selected = to
            .as_ref()
            .filter(|o| o.matches_tool(self.tool))
            .map(|_| index);
    }
    pub fn undo(&mut self) {
        self.finish_parameter_adjustment();
        self.cancel();
        if let Some(c) = self.undo.pop_back() {
            self.apply(c.index, &c.after, &c.before);
            self.redo.push(c);
        }
    }
    pub fn redo(&mut self) {
        self.finish_parameter_adjustment();
        self.cancel();
        if let Some(c) = self.redo.pop() {
            self.apply(c.index, &c.before, &c.after);
            self.undo.push_back(c);
        }
    }
    pub fn delete(&mut self) {
        self.finish_parameter_adjustment();
        self.cancel();
        if let Some(index) = self.selected.take() {
            let before = self.objects.remove(index);
            self.record(Change {
                index,
                before: Some(before),
                after: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(x: i32, y: i32) -> PixelPoint {
        PixelPoint { x, y }
    }

    #[test]
    fn mosaic_interior_and_all_eight_handles_edit_independently_of_granularity() {
        let mut editor = Editor {
            tool: Tool::Mosaic,
            ..Default::default()
        };
        let bounds = rect(p(-90, -50), p(-10, 30));
        assert!(editor.begin_at(p(bounds.left, bounds.top), None).unwrap());
        editor.finish(p(bounds.right, bounds.bottom));
        let initial = editor.objects()[0].clone();
        for (index, handle) in initial.handles().into_iter().enumerate() {
            let hit = editor.hit_at(handle, 5, 1.25, true);
            assert_eq!(hit, Some(ObjectHit::Handle(0, index)));
            assert!(editor.begin_at(handle, hit).unwrap());
            editor.finish(p(handle.x + 11, handle.y + 11));
            assert_ne!(editor.objects()[0].bounds(), bounds);
            editor.undo();
            assert_eq!(editor.objects()[0], initial);
        }
        let center = p(-50, -10);
        let hit = editor.hit_at(center, 5, 1.25, true);
        assert_eq!(hit, Some(ObjectHit::Move(0)));
        assert!(editor.begin_at(center, hit).unwrap());
        editor.set_mosaic_size(22);
        editor.finish(p(-40, 10));
        let moved = rect(p(-80, -30), p(0, 50));
        assert_eq!(editor.mosaics().next(), Some((moved, 22)));
        editor.set_mosaic_size(23);
        editor.finish_parameter_adjustment();
        assert_eq!(editor.mosaics().next(), Some((moved, 23)));
        editor.undo();
        assert_eq!(editor.mosaics().next(), Some((moved, 22)));
        editor.undo();
        assert_eq!(editor.mosaics().next(), Some((bounds, DEFAULT_MOSAIC_SIZE)));
        assert_eq!(editor.mosaic_size, 23);
        editor.redo();
        editor.redo();
        editor.delete();
        assert_eq!(editor.mosaics().next(), None);
        editor.undo();
        assert_eq!(editor.mosaics().next(), Some((moved, 23)));
        editor.redo();
        assert_eq!(editor.mosaics().next(), None);
    }
    fn text(content: &str) -> Object {
        Object {
            shape: Shape::Text {
                bounds: PixelRect {
                    left: -100,
                    top: -50,
                    right: 100,
                    bottom: 50,
                },
                content: content.into(),
                size: 30.,
                wrap_width: 200.,
                angle: 0.,
            },
            style: Style::default(),
        }
    }
    #[test]
    fn multiline_text_reedit_delete_and_history_roundtrip() {
        let mut e = Editor::default();
        e.commit_text(None, text("中文\n第二行😀")).unwrap();
        e.commit_text(Some(0), text("修改后的中文")).unwrap();
        assert_eq!(e.undo.len(), 2);
        e.undo();
        assert_eq!(e.objects()[0], text("中文\n第二行😀"));
        e.redo();
        assert_eq!(e.objects()[0], text("修改后的中文"));
        e.commit_text(Some(0), text("")).unwrap();
        assert!(e.objects().is_empty());
        e.undo();
        assert_eq!(e.objects()[0], text("修改后的中文"));
    }
    #[test]
    fn rotate_text_hit_and_history_use_the_rotated_bounds() {
        let mut e = Editor::default();
        e.commit_text(None, text("旋转")).unwrap();
        let before = e.objects()[0].clone();
        let bounds = before.bounds();
        let (x, y) = text_center(bounds);
        let handle = before.rotation_handle(1.).unwrap();
        let rotated_handle = p(
            (x + y - f64::from(handle.y)).round() as i32,
            y.round() as i32,
        );
        e.begin(handle, 5, 1.).unwrap();
        e.finish(rotated_handle);
        let after = e.objects()[0].clone();
        assert!(
            matches!(after.shape, Shape::Text { angle, .. } if (angle - std::f64::consts::FRAC_PI_2).abs() < 0.06)
        );
        assert_eq!(after.bounds(), bounds);
        assert_eq!(e.text_at(p(x as i32, y as i32)), Some(0));
        assert_eq!(e.text_at(p(bounds.left + 1, y as i32)), None);
        e.undo();
        assert_eq!(e.objects(), &[before]);
        e.redo();
        assert_eq!(e.objects(), &[after]);
    }

    #[test]
    fn text_validation_failure_preserves_object_and_redo() {
        let mut e = Editor::default();
        e.commit_text(None, text("原文")).unwrap();
        e.commit_text(Some(0), text("新文")).unwrap();
        e.undo();
        assert!(e.commit_text(Some(10), text("错误索引")).is_err());
        assert!(e.commit_text(Some(0), text(&"字".repeat(16385))).is_err());
        assert_eq!(e.objects()[0], text("原文"));
        assert!(e.can_redo());
    }
    #[test]
    fn text_moves_without_reflow_and_topmost_shape_blocks_text_hit() {
        let mut e = Editor::default();
        e.commit_text(None, text("中文\n多行")).unwrap();
        e.begin(p(0, 0), 6, 1.).unwrap();
        e.finish(p(50, 20));
        assert!(
            matches!(&e.objects()[0].shape,Shape::Text{bounds,wrap_width,content,..} if bounds.left==-50&&*wrap_width==200.&&content=="中文\n多行")
        );
        draw(&mut e, Tool::Arrow, p(0, 0), p(100, 0));
        assert_eq!(e.text_at(p(50, 0)), None);
    }
    fn draw(e: &mut Editor, tool: Tool, a: PixelPoint, b: PixelPoint) {
        e.tool = tool;
        assert!(e.begin(a, 6, 1.).unwrap());
        e.finish(b);
    }
    #[test]
    fn gesture_history_cancel_and_redo_branch() {
        let mut e = Editor {
            tool: Tool::Rectangle,
            ..Default::default()
        };
        e.begin(p(-100, -100), 6, 1.).unwrap();
        for i in 1..=50 {
            e.update(p(i, i));
        }
        e.finish(p(50, 50));
        assert_eq!(e.undo.len(), 1);
        e.tool = Tool::Select;
        e.begin(p(-100, -100), 6, 1.).unwrap();
        e.update(p(-200, -200));
        assert!(e.cancel());
        assert_eq!(e.objects[0].bounds(), rect(p(-100, -100), p(50, 50)));
        e.undo();
        assert!(e.objects.is_empty());
        e.redo();
        assert_eq!(e.objects.len(), 1);
        e.delete();
        e.undo();
        assert_eq!(e.objects.len(), 1);
        draw(&mut e, Tool::Arrow, p(0, 100), p(100, 0));
        assert!(!e.can_redo());
    }
    #[test]
    fn topmost_hit_arrow_endpoint_and_whole_stroke_move() {
        let mut e = Editor::default();
        draw(&mut e, Tool::Arrow, p(0, 0), p(100, 100));
        draw(&mut e, Tool::Pen, p(0, 0), p(100, 100));
        e.tool = Tool::Select;
        e.selected = None;
        e.begin(p(50, 50), 6, 1.).unwrap();
        assert_eq!(e.selected, Some(1));
        e.finish(p(70, 60));
        assert_eq!(e.objects[1].shape, Shape::Pen(vec![p(20, 10), p(120, 110)]));
        e.undo();
        e.delete();
        e.selected = Some(0);
        e.begin(p(100, 100), 6, 1.).unwrap();
        e.finish(p(150, 90));
        assert_eq!(e.objects[0].shape, Shape::Arrow(p(0, 0), p(150, 90)));
    }
    #[test]
    fn arrow_keeps_start_in_four_directions_and_pen_keeps_release() {
        let start = p(-40, 20);
        for end in [p(50, -50), p(50, 50), p(-100, -50), p(-100, 50)] {
            let mut e = Editor {
                tool: Tool::Arrow,
                ..Default::default()
            };
            assert!(e.begin(start, 5, 1.).unwrap());
            e.update(p(0, 0));
            e.finish(end);
            assert_eq!(e.objects().last().unwrap().shape, Shape::Arrow(start, end));
        }
        let mut e = Editor {
            tool: Tool::Pen,
            ..Default::default()
        };
        e.begin(start, 5, 1.).unwrap();
        e.update(start);
        e.finish(p(1, 4));
        assert_eq!(
            e.objects().last().unwrap().shape,
            Shape::Pen(vec![start, p(1, 4)])
        );
        let dot = p(100, 100);
        e.begin(dot, 5, 1.).unwrap();
        e.finish(dot);
        assert_eq!(e.objects().last().unwrap().shape, Shape::Pen(vec![dot]));
    }

    #[test]
    fn arrow_wings_are_hittable_and_inside_bounds() {
        let object = Object {
            shape: Shape::Arrow(p(0, 0), p(100, 0)),
            style: Style {
                width: 6.,
                ..Default::default()
            },
        };
        for wing in arrow_head(p(0, 0), p(100, 0), 6.) {
            assert!(object.hit(wing, 0.));
            let r = object.bounds();
            assert!(wing.x >= r.left && wing.x <= r.right && wing.y >= r.top && wing.y <= r.bottom);
        }
    }
    #[test]
    fn styles_keep_physical_width_and_edits_are_undoable() {
        let mut e = Editor {
            tool: Tool::Arrow,
            ..Default::default()
        };
        e.begin(p(0, 0), 6, 1.5).unwrap();
        e.finish(p(100, 100));
        assert_eq!(e.objects()[0].style.width, 3.);
        e.set_stroke_width(6);
        e.set_stroke_width(64);
        assert_eq!(e.objects()[0].style.width, 64.);
        e.finish_parameter_adjustment();
        e.set_stroke_color([0, 122, 255]);
        assert_eq!(e.objects()[0].style.width, 64.);
        e.undo();
        e.undo();
        assert_eq!(e.objects()[0].style, Style::default());
        assert_eq!(e.style().width, 3.);
        assert_eq!(e.styles[1].width, 64.);
        e.redo();
        e.redo();
        assert_eq!(e.objects()[0].style.color, [0, 122, 255]);
        e.begin(p(200, 100), 6, 2.).unwrap();
        e.finish(p(300, 200));
        assert_eq!(e.objects()[1].style.width, 64.);
    }
    #[test]
    fn width_changes_during_creation_and_cancel_keep_tool_defaults_independent() {
        let mut e = Editor {
            tool: Tool::Pen,
            ..Default::default()
        };
        e.begin(p(0, 0), 6, 2.).unwrap();
        e.update(p(35, 0));
        e.set_stroke_width(5);
        e.set_stroke_width(17);
        e.finish(p(75, 0));
        assert_eq!(e.objects()[0].style.width, 17.);
        assert_eq!(e.undo.len(), 1);
        e.undo();
        assert!(e.objects().is_empty());
        assert_eq!(e.style().width, 17.);
        e.redo();
        e.begin(p(35, 0), 6, 2.).unwrap();
        e.set_stroke_width(31);
        assert!(e.cancel());
        assert_eq!(e.objects()[0].style.width, 17.);
        assert_eq!(e.styles[2].width, 31.);
        e.tool = Tool::Rectangle;
        assert_eq!(e.style().width, 3.);
    }
    #[test]
    fn long_stroke_limit_is_explicit_and_extreme_move_does_not_overflow() {
        let mut e = Editor {
            tool: Tool::Pen,
            ..Default::default()
        };
        e.begin(p(0, 0), 6, 1.).unwrap();
        for i in 1..17000 {
            e.update(p(i, 1));
        }
        e.finish(p(18000, 1));
        assert!(e.take_warning().is_some());
        assert!(matches!(&e.objects()[0].shape,Shape::Pen(points) if points.len()==16384));
        e.tool = Tool::Select;
        e.begin(p(100, 1), 6, 1.).unwrap();
        e.finish(p(i32::MAX, i32::MIN));
        assert_eq!(e.objects()[0].bounds().right, i32::MAX);
        e.undo();
        assert_eq!(e.objects()[0].bounds().left, 0);
    }
    #[test]
    fn bounded_history_and_rectangle_crossing() {
        let mut e = Editor::default();
        draw(&mut e, Tool::Rectangle, p(-20, -10), p(20, 10));
        e.tool = Tool::Select;
        e.begin(p(-20, -10), 6, 1.).unwrap();
        e.finish(p(40, 30));
        assert_eq!(e.objects[0].bounds(), rect(p(20, 10), p(40, 30)));
        for i in 0..105 {
            draw(&mut e, Tool::Arrow, p(i * 20, 0), p(i * 20, 10));
        }
        assert_eq!(e.undo.len(), 100);
        for _ in 0..100 {
            e.undo();
        }
        assert_eq!(e.objects.len(), 6);
    }

    #[test]
    fn same_tool_edits_topmost_match_and_allows_nested_rectangles() {
        let mut e = Editor::default();
        draw(&mut e, Tool::Rectangle, p(0, 0), p(100, 100));
        draw(&mut e, Tool::Rectangle, p(25, 40), p(80, 100));
        draw(&mut e, Tool::Pen, p(50, 90), p(50, 115));
        e.tool = Tool::Rectangle;
        assert_eq!(e.hit_at(p(65, 100), 6, 1., true), Some(ObjectHit::Move(1)));
        assert!(e.begin(p(65, 100), 6, 1.).unwrap());
        e.update(p(70, 105));
        e.finish(p(75, 110));
        assert_eq!(e.objects()[0].bounds(), rect(p(0, 0), p(100, 100)));
        assert_eq!(e.objects()[1].bounds(), rect(p(35, 50), p(90, 110)));
        assert_eq!(e.undo.len(), 4);
        e.undo();
        assert_eq!(e.objects()[1].bounds(), rect(p(25, 40), p(80, 100)));
        e.redo();
        assert_eq!(e.objects()[1].bounds(), rect(p(35, 50), p(90, 110)));
    }

    #[test]
    fn direct_handles_preserve_grab_offset_and_text_uses_rotated_local_bounds() {
        let mut e = Editor::default();
        draw(&mut e, Tool::Arrow, p(0, 0), p(100, 0));
        e.selected = None;
        assert_eq!(
            e.hit_at(p(98, 2), 6, 1., true),
            Some(ObjectHit::Handle(0, 1))
        );
        e.begin(p(98, 2), 6, 1.).unwrap();
        e.finish(p(98, 2));
        assert_eq!(e.undo.len(), 1);
        e.begin(p(98, 2), 6, 1.).unwrap();
        e.finish(p(-12, 22));
        assert_eq!(e.objects()[0].shape, Shape::Arrow(p(0, 0), p(-10, 20)));

        e.tool = Tool::Text;
        let mut object = text("中文\n两行");
        let Shape::Text {
            bounds,
            ref mut angle,
            ..
        } = object.shape
        else {
            unreachable!();
        };
        *angle = std::f64::consts::FRAC_PI_2;
        e.commit_text(None, object.clone()).unwrap();
        let (cx, cy) = text_center(bounds);
        let center = p(cx.round() as i32, cy.round() as i32);
        let (x, y) = rotate_point(
            bounds,
            std::f64::consts::FRAC_PI_2,
            cx,
            f64::from(bounds.top),
        );
        let border = p(x.round() as i32, y.round() as i32);
        assert_eq!(e.hit_at(center, 6, 1., true), Some(ObjectHit::Text(1)));
        assert_eq!(e.hit_at(border, 6, 1., true), Some(ObjectHit::Move(1)));
        assert_eq!(
            e.hit_at(p(border.x + 7, border.y), 6, 1., true),
            Some(ObjectHit::Move(1))
        );
        let handle = object.rotation_handle(1.).unwrap();
        assert_eq!(e.hit_at(handle, 6, 1., false), Some(ObjectHit::Rotate(1)));
        e.begin(border, 6, 1.).unwrap();
        e.update(p(border.x + 12, border.y + 24));
        assert!(e.cancel());
        assert_eq!(e.objects()[1], object);
    }
}
