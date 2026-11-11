//! Shared physical bounds for painting, hit testing and keyboard navigation.
use crate::core::{
    annotation::Tool,
    geometry::PixelRect,
    selection::{PixelPoint, contains, toolbar},
};

pub const MORE: usize = 21;
pub const BUTTON_HEIGHT: f32 = 36.;
const BAR_HEIGHT: f32 = 48.;
const PARAM_HEIGHT: f32 = 42.;
const PARAM_WIDTH: f32 = 506.;
const BUTTON_GAP: f32 = 4.;
const DRAWING: &[usize] = &[0, 1, 2, 3, 4];
const HISTORY: &[usize] = &[5, 6];
const OUTPUT: &[usize] = &[7, 8, 9];
const CANCEL: &[usize] = &[10];
const FULL: &[&[usize]] = &[DRAWING, HISTORY, OUTPUT, CANCEL];
const COLLAPSED_DRAWING: &[&[usize]] = &[&[0, MORE], HISTORY, OUTPUT, CANCEL];
const COLLAPSED_ALL: &[&[usize]] = &[&[MORE], OUTPUT, CANCEL];

pub fn tool_visible(drawing_tools: [bool; 4], id: usize) -> bool {
    match id {
        1..=4 => drawing_tools[id - 1],
        _ => true,
    }
}

pub fn name(id: usize) -> &'static str {
    match id {
        0 => Tool::Select.label(),
        1..=4 => Tool::DRAWING[id - 1].label(),
        5 => "撤销（Ctrl+Z）",
        6 => "重做（Ctrl+Y）",
        7 => "贴图",
        8 => "保存（Ctrl+S）",
        9 => "复制",
        10 => "取消（Esc）",
        MORE => "更多",
        _ => "",
    }
}

fn width(id: usize) -> f32 {
    if id == 9 { 83. } else { 42. }
}

fn bar_width(
    groups: &[&[usize]],
    visible: impl Fn(usize) -> bool,
    padding: f32,
    group_gap: f32,
) -> f32 {
    let buttons = groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .filter(|&id| visible(id));
    buttons.map(|id| width(id) + BUTTON_GAP).sum::<f32>()
        + 2. * padding
        + (group_gap - BUTTON_GAP) * (groups.len() - 1) as f32
        - BUTTON_GAP
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
    buttons: [Option<Button>; 11],
    dividers: [Option<i32>; 3],
    overflow: [Option<usize>; 7],
    pub scale: f32,
    pub parameter_scale: f32,
}

impl Layout {
    pub fn new(
        rect: PixelRect,
        screen: PixelRect,
        preferred: f32,
        parameters: bool,
        drawing_tools: [bool; 4],
    ) -> Self {
        let visible = |id| tool_visible(drawing_tools, id);
        let available = (i64::from(screen.right) - i64::from(screen.left)) as f32;
        let available_height = (i64::from(screen.bottom) - i64::from(screen.top)) as f32;
        let mut groups = FULL;
        let mut padding = 18.;
        let mut group_gap = 18.;
        let mut overflow = [None; 7];
        if bar_width(FULL, visible, padding, group_gap) * preferred > available {
            if drawing_tools.into_iter().any(|enabled| enabled) {
                groups = COLLAPSED_DRAWING;
                for (slot, id) in overflow.iter_mut().zip((1..=4).filter(|&id| visible(id))) {
                    *slot = Some(id);
                }
            }
            if bar_width(groups, visible, padding, group_gap) * preferred > available {
                groups = COLLAPSED_ALL;
                padding = 12.;
                group_gap = 14.;
                overflow = std::array::from_fn(|id| visible(id).then_some(id));
            }
        }
        let bar_dip = bar_width(groups, visible, padding, group_gap);
        let mut scale = preferred.min(available / bar_dip);
        let mut parameter_scale = preferred.min(available / PARAM_WIDTH);
        let mut height = BAR_HEIGHT * scale;
        if parameters {
            height += 8. * scale + PARAM_HEIGHT * parameter_scale;
        }
        let fit = (available_height / height).min(1.);
        scale *= fit;
        parameter_scale *= fit;
        let bar_px = (bar_dip * scale).round() as i32;
        let parameter_px = if parameters {
            (PARAM_WIDTH * parameter_scale).round() as i32
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
        let mut buttons = [None; 11];
        let mut button_count = 0;
        let mut dividers = [None; 3];
        for (group_index, group) in groups.iter().enumerate() {
            if group_index != 0 {
                dividers[group_index - 1] = Some(x(offset - BUTTON_GAP + group_gap / 2.));
                offset += group_gap - BUTTON_GAP;
            }
            for id in group.iter().copied().filter(|&id| visible(id)) {
                let button_width = width(id);
                buttons[button_count] = Some(Button {
                    id,
                    bounds: PixelRect {
                        left: x(offset),
                        right: x(offset + button_width),
                        top: y((BAR_HEIGHT - BUTTON_HEIGHT) / 2.),
                        bottom: y((BAR_HEIGHT + BUTTON_HEIGHT) / 2.),
                    },
                });
                button_count += 1;
                offset += button_width + BUTTON_GAP;
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
        self.parameter_box(305., 418., 8.)
    }

    pub fn parameter_edit(&self) -> Option<PixelRect> {
        self.parameter_box(430., 477., 7.)
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
            for mask in 0u32..16 {
                let tools = std::array::from_fn(|i| mask & (1 << i) != 0);
                let layout = Layout::new(selection, display, scale, false, tools);
                let expected: Vec<_> = (0..=10).filter(|&id| tool_visible(tools, id)).collect();
                assert_eq!(layout.buttons().map(|b| b.id).collect::<Vec<_>>(), expected);
                assert_eq!(layout.dividers().count(), 3);
                assert_eq!(layout.overflow().count(), 0);
                assert_eq!(layout.toolbar.intersection(display), Some(layout.toolbar));
                let width = ((621 - (4 - mask.count_ones()) * 46) as f32 * scale).round() as i32;
                assert_eq!(layout.toolbar.right - layout.toolbar.left, width);
                for button in layout.buttons() {
                    let b = button.bounds;
                    assert_eq!(
                        layout.button_at(PixelPoint {
                            x: (b.left + b.right) / 2,
                            y: (b.top + b.bottom) / 2
                        }),
                        Some(button.id),
                    );
                }
            }
        }
    }

    #[test]
    fn narrow_display_overflow_keeps_full_size_output_and_has_no_hidden_tools() {
        let tools = [true, false, true, true];
        let selection = rect(-320, 700, 470, 200);
        let medium = Layout::new(selection, rect(-1020, 0, 1000, 900), 2., false, [true; 4]);
        assert_eq!(medium.overflow().collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!(
            medium.buttons().map(|b| b.id).collect::<Vec<_>>(),
            [0, MORE, 5, 6, 7, 8, 9, 10]
        );

        let display = rect(-660, 0, 640, 900);
        let narrow = Layout::new(selection, display, 2., true, tools);
        assert_eq!(narrow.overflow().collect::<Vec<_>>(), [0, 1, 3, 4, 5, 6]);
        assert_eq!(
            narrow.buttons().map(|b| b.id).collect::<Vec<_>>(),
            [MORE, 7, 8, 9, 10]
        );
        assert_eq!(narrow.dividers().count(), 2);
        assert_eq!(narrow.bounds.intersection(display), Some(narrow.bounds));
        assert!(narrow.bounds.bottom <= selection.top);
        assert!(narrow.parameter_scale < narrow.scale);
        for button in narrow.buttons() {
            let b = button.bounds;
            assert_eq!(b.right - b.left, if button.id == 9 { 166 } else { 84 });
            assert_eq!(b.bottom - b.top, 72);
        }
    }
}
