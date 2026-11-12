//! Shared physical bounds for painting, hit testing and keyboard navigation.
use crate::core::{
    annotation::{Editor, Tool},
    geometry::PixelRect,
    selection::{PixelPoint, contains, toolbar},
};

pub const UNDO: usize = 5;
pub const COPY: usize = 9;
pub const MORE: usize = 21;
pub const MOSAIC: usize = 22;
pub const SEQUENCE: usize = 23;
pub const PICKER: usize = 24;
pub const LONG_CAPTURE: usize = 25;
pub const BUTTON_HEIGHT: f32 = 36.;
pub const BAR_HEIGHT: f32 = 48.;
const PARAM_HEIGHT: f32 = 42.;
const PARAM_WIDTH: f32 = 506.;
const BUTTON_WIDTH: f32 = 42.;
const BUTTON_GAP: f32 = 4.;
const TOOLS: [usize; Tool::TOOLBAR.len()] = [1, 2, 3, 4, MOSAIC, SEQUENCE, LONG_CAPTURE, PICKER];
const DRAWING: &[usize] = TOOLS.split_at(TOOLS.len() - 2).0;
const UTILITIES: &[usize] = &[LONG_CAPTURE, PICKER, UNDO];
const UNDO_ONLY: &[usize] = &[UNDO];
const OUTPUT: &[usize] = &[7, 8, COPY];
const CANCEL: &[usize] = &[10];
const FULL: &[&[usize]] = &[DRAWING, UTILITIES, OUTPUT, CANCEL];
const COLLAPSED: &[&[usize]] = &[&[MORE], UNDO_ONLY, OUTPUT, CANCEL];
const MAX_BUTTONS: usize = TOOLS.len() + UNDO_ONLY.len() + OUTPUT.len() + CANCEL.len();

pub fn tool_button(tool: Tool) -> usize {
    tool.index().map_or(0, |index| TOOLS[index])
}

pub fn toolbar_tool(id: usize) -> Option<Tool> {
    TOOLS
        .iter()
        .position(|&button| button == id)
        .map(|index| Tool::TOOLBAR[index])
}

pub fn tool_visible(capture_tools: [bool; Tool::TOOLBAR.len()], id: usize) -> bool {
    TOOLS
        .iter()
        .position(|&button| button == id)
        .is_none_or(|index| capture_tools[index])
}

pub fn enabled(editor: &Editor, id: usize) -> bool {
    match id {
        UNDO => editor.can_undo(),
        11..=18 => editor.tool.has_color(),
        19..=20 => editor.tool.parameter().is_some(),
        7..=10 | MORE => true,
        _ => toolbar_tool(id).is_some(),
    }
}

pub fn name(id: usize) -> &'static str {
    match id {
        UNDO => "撤销（Ctrl+Z）",
        7 => "贴图",
        8 => "保存（Ctrl+S）",
        COPY => "复制（Enter）",
        10 => "取消（Esc）",
        MORE => "更多",
        _ => toolbar_tool(id).map_or("", Tool::label),
    }
}

fn bar_width(
    groups: &[&[usize]],
    visible: impl Fn(usize) -> bool,
    padding: f32,
    group_gap: f32,
) -> f32 {
    let mut groups = groups
        .iter()
        .map(|group| {
            group.iter().copied().filter(|&id| visible(id)).count() as f32
                * (BUTTON_WIDTH + BUTTON_GAP)
        })
        .filter(|&width| width > 0.);
    let first = groups.next().unwrap_or(0.) - BUTTON_GAP;
    first
        + 2. * padding
        + groups
            .map(|width| width + group_gap - BUTTON_GAP)
            .sum::<f32>()
}

#[derive(Clone, Copy)]
pub struct Button {
    pub id: usize,
    pub bounds: PixelRect,
}

pub struct Layout {
    pub bounds: PixelRect,
    pub toolbar: PixelRect,
    pub parameters: Option<PixelRect>,
    pub screen: PixelRect,
    buttons: [Option<Button>; MAX_BUTTONS],
    dividers: [Option<i32>; FULL.len() - 1],
    overflow: [Option<usize>; TOOLS.len()],
    pub scale: f32,
    pub parameter_scale: f32,
    parameter_tool: Tool,
}

impl Layout {
    pub fn new(
        rect: PixelRect,
        screen: PixelRect,
        preferred: f32,
        tool: Tool,
        capture_tools: [bool; Tool::TOOLBAR.len()],
    ) -> Self {
        Self::build(rect, screen, preferred, tool, capture_tools, true)
    }

    pub fn preview(bounds: PixelRect, capture_tools: [bool; Tool::TOOLBAR.len()]) -> Self {
        Self::build(bounds, bounds, 1., Tool::Select, capture_tools, false)
    }

    fn build(
        rect: PixelRect,
        screen: PixelRect,
        preferred: f32,
        tool: Tool,
        capture_tools: [bool; Tool::TOOLBAR.len()],
        allow_overflow: bool,
    ) -> Self {
        let visible = |id| tool_visible(capture_tools, id);
        let available = (i64::from(screen.right) - i64::from(screen.left)) as f32;
        let available_height = (i64::from(screen.bottom) - i64::from(screen.top)) as f32;
        let mut groups = FULL;
        let padding = 18.;
        let group_gap = 18.;
        let mut overflow = [None; TOOLS.len()];
        if allow_overflow
            && capture_tools.into_iter().any(|enabled| enabled)
            && bar_width(FULL, visible, padding, group_gap) * preferred > available
        {
            groups = COLLAPSED;
            for (slot, id) in overflow
                .iter_mut()
                .zip(TOOLS.iter().copied().filter(|&id| visible(id)))
            {
                *slot = Some(id);
            }
        }
        let bar_dip = bar_width(groups, visible, padding, group_gap);
        let mut scale = preferred.min(available / bar_dip);
        let parameter_width = match tool {
            Tool::Mosaic => 360.,
            Tool::Sequence => 415.,
            _ => PARAM_WIDTH,
        };
        let parameters = tool.parameter().is_some();
        let mut parameter_scale = preferred.min(available / parameter_width);
        let mut height = BAR_HEIGHT * scale;
        if parameters {
            height += 8. * scale + PARAM_HEIGHT * parameter_scale;
        }
        let fit = (available_height / height).min(1.);
        scale *= fit;
        parameter_scale *= fit;
        let bar_px = (bar_dip * scale).round() as i32;
        let parameter_px = if parameters {
            (parameter_width * parameter_scale).round() as i32
        } else {
            0
        };
        let area = toolbar(
            rect,
            screen,
            bar_px.max(parameter_px),
            (height * fit).round() as i32,
            (10. * scale).round() as i32,
        );
        let center = |width: i32| area.left + (area.right - area.left - width) / 2;
        let left = center(bar_px);
        let x = |offset: f32| left + (offset * scale).round() as i32;
        let y = |offset: f32| area.top + (offset * scale).round() as i32;
        let mut offset = padding;
        let mut buttons = [None; MAX_BUTTONS];
        let mut button_count = 0;
        let mut dividers = [None; FULL.len() - 1];
        let present = groups
            .iter()
            .filter(|group| group.iter().any(|&id| visible(id)));
        for (group_index, group) in present.enumerate() {
            if group_index != 0 {
                dividers[group_index - 1] = Some(x(offset - BUTTON_GAP + group_gap / 2.));
                offset += group_gap - BUTTON_GAP;
            }
            for id in group.iter().copied().filter(|&id| visible(id)) {
                buttons[button_count] = Some(Button {
                    id,
                    bounds: PixelRect {
                        left: x(offset),
                        right: x(offset + BUTTON_WIDTH),
                        top: y((BAR_HEIGHT - BUTTON_HEIGHT) / 2.),
                        bottom: y((BAR_HEIGHT + BUTTON_HEIGHT) / 2.),
                    },
                });
                button_count += 1;
                offset += BUTTON_WIDTH + BUTTON_GAP;
            }
        }
        let param_left = center(parameter_px);
        Self {
            bounds: area,
            toolbar: PixelRect {
                left,
                right: left + bar_px,
                bottom: y(BAR_HEIGHT),
                ..area
            },
            parameters: parameters.then_some(PixelRect {
                left: param_left,
                right: param_left + parameter_px,
                top: y(BAR_HEIGHT + 8.),
                bottom: area.bottom,
            }),
            screen,
            buttons,
            dividers,
            overflow,
            scale,
            parameter_scale,
            parameter_tool: tool,
        }
    }

    pub fn button_at(&self, point: PixelPoint) -> Option<usize> {
        self.buttons()
            .find(|b| contains(b.bounds, point))
            .map(|b| b.id)
    }

    pub fn buttons(&self) -> impl Iterator<Item = &Button> {
        self.buttons.iter().flatten()
    }

    pub fn dividers(&self) -> impl Iterator<Item = i32> + '_ {
        self.dividers.iter().flatten().copied()
    }

    pub fn overflow(&self) -> impl Iterator<Item = usize> + '_ {
        self.overflow.iter().flatten().copied()
    }

    fn parameter_box(&self, left: f32, right: f32, inset_y: f32) -> Option<PixelRect> {
        let r = self.parameters?;
        let px = |value: f32| (value * self.parameter_scale).round() as i32;
        Some(PixelRect {
            left: r.left + px(left),
            right: r.left + px(right),
            top: r.top + px(inset_y),
            bottom: r.bottom - px(inset_y),
        })
    }

    pub fn parameter_slider(&self) -> Option<PixelRect> {
        let (left, right) = match self.parameter_tool {
            Tool::Mosaic => (94., 270.),
            Tool::Sequence => (160., 325.),
            _ => (305., 418.),
        };
        self.parameter_box(left, right, 8.)
    }

    pub fn parameter_edit(&self) -> Option<PixelRect> {
        let (left, right) = match self.parameter_tool {
            Tool::Mosaic => (282., 329.),
            Tool::Sequence => (337., 384.),
            _ => (430., 477.),
        };
        self.parameter_box(left, right, 7.)
    }

    pub fn color_parameters(&self) -> bool {
        self.parameter_tool.has_color()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, width: i32, height: i32) -> PixelRect {
        PixelRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    #[test]
    fn every_visibility_combination_compacts_geometry_and_keeps_fixed_actions() {
        let display = rect(-2200, -900, 2000, 1200);
        let selection = rect(-2100, 10, 2020, 275);
        for scale in [1., 1.25, 1.5, 2.] {
            for mask in 0u32..1 << Tool::TOOLBAR.len() {
                let tools = std::array::from_fn(|i| mask & (1 << i) != 0);
                let layout = Layout::new(selection, display, scale, Tool::Select, tools);
                let expected: Vec<_> = [
                    1,
                    2,
                    3,
                    4,
                    MOSAIC,
                    SEQUENCE,
                    LONG_CAPTURE,
                    PICKER,
                    UNDO,
                    7,
                    8,
                    COPY,
                    10,
                ]
                .into_iter()
                .filter(|&id| tool_visible(tools, id))
                .collect();
                assert_eq!(layout.buttons().map(|b| b.id).collect::<Vec<_>>(), expected);
                let drawing_group = usize::from(tools[..6].iter().any(|&enabled| enabled));
                assert_eq!(layout.dividers().count(), 2 + drawing_group);
                assert_eq!(layout.overflow().count(), 0);
                assert_eq!(layout.toolbar.intersection(display), Some(layout.toolbar));
                let width = ((290 + mask.count_ones() * 46 + drawing_group as u32 * 14) as f32
                    * scale)
                    .round() as i32;
                assert_eq!(layout.toolbar.right - layout.toolbar.left, width);
                for button in layout.buttons() {
                    let b = button.bounds;
                    assert!(((b.right - b.left) as f32 - BUTTON_WIDTH * scale).abs() < 1.);
                    assert_eq!(
                        layout.button_at(PixelPoint {
                            x: (b.left + b.right) / 2,
                            y: (b.top + b.bottom) / 2
                        }),
                        Some(button.id),
                    );
                }
                let preview = Layout::preview(rect(0, 0, 720, 48), tools);
                assert_eq!(
                    preview.buttons().map(|b| b.id).collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(preview.dividers().count(), 2 + drawing_group);

                for bar in [&layout, &preview] {
                    let utilities = bar
                        .buttons()
                        .filter(|button| matches!(button.id, LONG_CAPTURE | PICKER | UNDO))
                        .collect::<Vec<_>>();
                    for pair in utilities.windows(2) {
                        let (left, right) = (pair[0].bounds.right, pair[1].bounds.left);
                        assert!(((right - left) as f32 - BUTTON_GAP * bar.scale).abs() < 1.);
                        assert!(!bar.dividers().any(|x| (left..=right).contains(&x)));
                    }
                }
            }
        }
    }

    #[test]
    fn narrow_display_overflow_keeps_full_size_output_and_has_no_hidden_tools() {
        let tools = [true, false, true, true, true, false, false, false];
        let selection = rect(-320, 700, 470, 200);
        let medium = Layout::new(
            selection,
            rect(-1020, 0, 1000, 900),
            2.,
            Tool::Select,
            [true; Tool::TOOLBAR.len()],
        );
        assert_eq!(
            medium.overflow().collect::<Vec<_>>(),
            [1, 2, 3, 4, MOSAIC, SEQUENCE, LONG_CAPTURE, PICKER]
        );
        assert_eq!(
            medium.buttons().map(|b| b.id).collect::<Vec<_>>(),
            [MORE, UNDO, 7, 8, COPY, 10]
        );

        let display = rect(-740, 0, 720, 900);
        let narrow = Layout::new(selection, display, 2., Tool::Rectangle, tools);
        assert_eq!(narrow.overflow().collect::<Vec<_>>(), [1, 3, 4, MOSAIC]);
        assert_eq!(
            narrow.buttons().map(|b| b.id).collect::<Vec<_>>(),
            [MORE, UNDO, 7, 8, COPY, 10]
        );
        assert_eq!(narrow.dividers().count(), 3);
        assert_eq!(narrow.bounds.intersection(display), Some(narrow.bounds));
        assert!(narrow.bounds.bottom <= selection.top);
        assert!(narrow.parameter_scale < narrow.scale);
        for button in narrow.buttons() {
            let b = button.bounds;
            assert_eq!(b.right - b.left, 84);
            assert_eq!(b.bottom - b.top, 72);
        }

        let settings = Layout::preview(rect(0, 0, 390, 48), tools);
        assert_eq!(
            settings.buttons().map(|b| b.id).collect::<Vec<_>>(),
            [1, 3, 4, MOSAIC, UNDO, 7, 8, COPY, 10]
        );
        assert_eq!(settings.overflow().count(), 0);
        assert!(settings.scale < 1.);

        let tiny = rect(-160, 0, 160, 200);
        let no_tools = Layout::new(
            selection,
            tiny,
            2.,
            Tool::Select,
            [false; Tool::TOOLBAR.len()],
        );
        assert_eq!(
            no_tools.buttons().map(|b| b.id).collect::<Vec<_>>(),
            [UNDO, 7, 8, COPY, 10]
        );
        assert_eq!(no_tools.overflow().count(), 0);
        assert_eq!(no_tools.toolbar.intersection(tiny), Some(no_tools.toolbar));
    }
}
