//! Editable annotations in global physical pixels; no native handles or image copies.
use super::{
    geometry::PixelRect,
    selection::{PixelPoint, Selection},
};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rectangle,
    Arrow,
    Pen,
    Text,
}
impl Tool {
    pub fn button(self) -> usize {
        match self {
            Self::Select => 0,
            Self::Rectangle => 1,
            Self::Arrow => 2,
            Self::Pen => 3,
            Self::Text => 4,
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
    pub fn bounds(&self) -> PixelRect {
        match &self.shape {
            Shape::Text { bounds, .. } => *bounds,
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
            Shape::Rectangle(r) => Selection::handles(*r).to_vec(),
            Shape::Arrow(a, b) => vec![*a, *b],
            Shape::Pen(_) | Shape::Text { .. } => Vec::new(),
        }
    }
    pub fn rotation_handle(&self, scale: f32) -> Option<PixelPoint> {
        let Shape::Text { bounds, angle, .. } = self.shape else {
            return None;
        };
        let (cx, _) = text_center(bounds);
        let (x, y) = rotate_point(
            bounds,
            angle,
            cx,
            f64::from(bounds.top) - 16. * f64::from(scale),
        );
        Some(PixelPoint {
            x: x.round() as i32,
            y: y.round() as i32,
        })
    }
    fn hit(&self, p: PixelPoint, tolerance: f64) -> bool {
        let t = tolerance + f64::from(self.style.width) / 2.;
        match &self.shape {
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

fn rect(a: PixelPoint, b: PixelPoint) -> PixelRect {
    PixelRect {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    }
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
    pub text_size: f32,
    drag: Option<Drag>,
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
            text_size: 24.,
            drag: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            warning: None,
        }
    }
}
impl Editor {
    pub fn text_at(&self, p: PixelPoint) -> Option<usize> {
        self.objects
            .iter()
            .rposition(|o| o.hit(p, 0.))
            .filter(|i| matches!(self.objects[*i].shape, Shape::Text { .. }))
    }
    pub fn commit_text(
        &mut self,
        index: Option<usize>,
        object: Object,
    ) -> Result<(), &'static str> {
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
            ..Default::default()
        }
    }
    pub fn objects(&self) -> &[Object] {
        &self.objects
    }
    pub fn active(&self) -> bool {
        self.drag.is_some()
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() && !self.active()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty() && !self.active()
    }
    pub fn enabled(&self, index: usize) -> bool {
        (matches!(index, 11..=21) && self.tool != Tool::Select)
            || matches!(index, 0..=4 | 7..=10)
            || (index == 5 && self.can_undo())
            || (index == 6 && self.can_redo())
    }
    pub fn style(&self) -> Style {
        self.styles[self.tool.button().saturating_sub(1).min(3)]
    }
    pub fn set_style(&mut self, style: Style, scale: f32) {
        if self.active() || !style.width.is_finite() || !(1. ..=32.).contains(&style.width) {
            return;
        }
        self.styles[self.tool.button().saturating_sub(1).min(3)] = style;
        if let Some(index) = self.selected {
            let before = self.objects[index].clone();
            self.objects[index].style = Style {
                width: style.width * scale.clamp(0.5, 4.),
                ..style
            };
            let after = self.objects[index].clone();
            self.record(Change {
                index,
                before: Some(before),
                after: Some(after),
            });
        }
    }
    pub fn rotation_at(&self, p: PixelPoint, tolerance: i32, scale: f32) -> bool {
        self.tool == Tool::Select
            && self
                .selected
                .and_then(|i| self.objects.get(i))
                .and_then(|o| o.rotation_handle(scale))
                .is_some_and(|handle| distance(p, handle, handle) <= f64::from(tolerance))
    }
    pub fn begin(
        &mut self,
        p: PixelPoint,
        tolerance: i32,
        scale: f32,
    ) -> Result<bool, &'static str> {
        self.cancel();
        self.warning = None;
        if self.tool == Tool::Select {
            if let Some(index) = self.selected {
                let object = &self.objects[index];
                if self.rotation_at(p, tolerance, scale) {
                    self.drag = Some(Drag {
                        index,
                        start: p,
                        before: Some(object.clone()),
                        kind: DragKind::Rotate,
                    });
                    return Ok(true);
                }
                if let Some(handle) = object
                    .handles()
                    .iter()
                    .position(|h| distance(p, *h, *h) <= f64::from(tolerance))
                {
                    self.drag = Some(Drag {
                        index,
                        start: p,
                        before: Some(object.clone()),
                        kind: DragKind::Handle(handle),
                    });
                    return Ok(true);
                }
            }
            self.selected = self
                .objects
                .iter()
                .rposition(|o| o.hit(p, f64::from(tolerance)));
            if let Some(index) = self.selected {
                self.drag = Some(Drag {
                    index,
                    start: p,
                    before: Some(self.objects[index].clone()),
                    kind: DragKind::Move,
                });
                return Ok(true);
            }
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
            Tool::Select | Tool::Text => return Ok(false),
        };
        let mut style = self.style();
        style.width *= scale.clamp(0.5, 4.);
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
                Shape::Rectangle(r) => *r = rect(drag.start, p),
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
                    object.shape = match &before.shape {
                        Shape::Rectangle(r) => Shape::Rectangle(rect(
                            shift(PixelPoint {
                                x: r.left,
                                y: r.top,
                            }),
                            shift(PixelPoint {
                                x: r.right,
                                y: r.bottom,
                            }),
                        )),
                        Shape::Text {
                            bounds,
                            content,
                            size,
                            wrap_width,
                            angle,
                        } => Shape::Text {
                            bounds: rect(
                                shift(PixelPoint {
                                    x: bounds.left,
                                    y: bounds.top,
                                }),
                                shift(PixelPoint {
                                    x: bounds.right,
                                    y: bounds.bottom,
                                }),
                            ),
                            content: content.clone(),
                            size: *size,
                            wrap_width: *wrap_width,
                            angle: *angle,
                        },
                        Shape::Arrow(a, b) => Shape::Arrow(shift(*a), shift(*b)),
                        Shape::Pen(points) => {
                            Shape::Pen(points.iter().map(|v| shift(*v)).collect())
                        }
                    };
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
                    match &before.shape {
                        Shape::Rectangle(original) => {
                            let mut r = *original;
                            if [0, 6, 7].contains(index) {
                                r.left = p.x;
                            }
                            if [2, 3, 4].contains(index) {
                                r.right = p.x;
                            }
                            if [0, 1, 2].contains(index) {
                                r.top = p.y;
                            }
                            if [4, 5, 6].contains(index) {
                                r.bottom = p.y;
                            }
                            object.shape = Shape::Rectangle(rect(
                                PixelPoint {
                                    x: r.left,
                                    y: r.top,
                                },
                                PixelPoint {
                                    x: r.right,
                                    y: r.bottom,
                                },
                            ));
                        }
                        Shape::Arrow(a, b) => {
                            object.shape = if *index == 0 {
                                Shape::Arrow(p, *b)
                            } else {
                                Shape::Arrow(*a, p)
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
            Shape::Text { bounds, .. } => bounds.size().is_some(),
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
        self.selected = to.as_ref().map(|_| index);
    }
    pub fn undo(&mut self) {
        self.cancel();
        if let Some(c) = self.undo.pop_back() {
            self.apply(c.index, &c.after, &c.before);
            self.redo.push(c);
        }
    }
    pub fn redo(&mut self) {
        self.cancel();
        if let Some(c) = self.redo.pop() {
            self.apply(c.index, &c.before, &c.after);
            self.undo.push_back(c);
        }
    }
    pub fn delete(&mut self) {
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
        let mut e = Editor::default();
        let start = p(-40, 20);
        for end in [p(50, -50), p(50, 50), p(-100, -50), p(-100, 50)] {
            e.tool = Tool::Arrow;
            assert!(e.begin(start, 5, 1.).unwrap());
            e.update(p(0, 0));
            e.finish(end);
            assert_eq!(e.objects().last().unwrap().shape, Shape::Arrow(start, end));
        }
        e.tool = Tool::Pen;
        e.begin(start, 5, 1.).unwrap();
        e.update(start);
        e.finish(p(1, 4));
        assert_eq!(
            e.objects().last().unwrap().shape,
            Shape::Pen(vec![start, p(1, 4)])
        );
        e.begin(start, 5, 1.).unwrap();
        e.finish(start);
        assert_eq!(e.objects().last().unwrap().shape, Shape::Pen(vec![start]));
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
        assert_eq!(e.objects()[0].style.width, 4.5);
        e.set_style(
            Style {
                color: [0, 122, 255],
                width: 6.,
            },
            1.5,
        );
        assert_eq!(e.objects()[0].style.width, 9.);
        e.undo();
        assert_eq!(
            e.objects()[0].style,
            Style {
                width: 4.5,
                ..Style::default()
            }
        );
        e.redo();
        assert_eq!(e.objects()[0].style.color, [0, 122, 255]);
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
            draw(&mut e, Tool::Arrow, p(i, 0), p(i, 10));
        }
        assert_eq!(e.undo.len(), 100);
        for _ in 0..100 {
            e.undo();
        }
        assert_eq!(e.objects.len(), 6);
    }
}
