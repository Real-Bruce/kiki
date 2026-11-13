use super::super::{composition, config_store::Disk, output, preferences, tests::take_ui_errors};
use super::*;
use crate::core::{
    config::{DEFAULT_SEQUENCE_SIZE, OutputFormat},
    display::ColorMode,
    image::Image,
};
use std::time::{Duration, Instant};
use windows::Win32::System::{DataExchange::*, Memory::*, Ole::CF_UNICODETEXT};

fn prepare_windows() -> composition::Apartment {
    super::super::tests::graphics_runtime();
    let apartment = composition::Apartment::enter().unwrap();
    // SAFETY: the registered classes live in this process, and the tests own their HWNDs.
    unsafe {
        let instance = GetModuleHandleW(None).unwrap().into();
        register(instance).unwrap();
        super::super::pinned::register(instance).unwrap();
    }
    apartment
}

fn capture_fixture(image: Image, screens: Vec<DisplayInfo>) -> (Preview, *mut State) {
    let bounds = image.bounds();
    let preview = Preview::open_inner(
        CaptureResult {
            image,
            screens,
            windows: Vec::new(),
            #[cfg(feature = "diagnostics")]
            elapsed: Duration::ZERO,
        },
        false,
    )
    .unwrap();
    // SAFETY: this test owns the hidden window and initializes its model before posting messages.
    unsafe {
        let p = GetWindowLongPtrW(preview.hwnd().unwrap(), GWLP_USERDATA) as *mut State;
        (*p).selection.rect = Some(bounds);
        (preview, p)
    }
}

fn send_mouse(hwnd: HWND, bounds: PixelRect, msg: u32, point: PixelPoint) {
    let x = u32::from((point.x - bounds.left) as u16);
    let y = u32::from((point.y - bounds.top) as u16);
    // SAFETY: hwnd belongs to this test thread; lparam packs client coordinates.
    unsafe {
        SendMessageW(hwnd, msg, None, Some(LPARAM((x | (y << 16)) as isize)));
    }
}

pub(super) fn pump_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if done() {
            return;
        }
        assert!(Instant::now() < deadline, "native operation timed out");
        // SAFETY: this test owns its message queue and dispatches call-scoped messages.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn click(hwnd: HWND, bounds: PixelRect, point: PixelPoint) {
    send_mouse(hwnd, bounds, WM_LBUTTONDOWN, point);
    send_mouse(hwnd, bounds, WM_LBUTTONUP, point);
}

fn point(x: i32, y: i32) -> PixelPoint {
    PixelPoint { x, y }
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PixelRect {
    PixelRect {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn numbering_drags_adjusts_persists_and_hides_through_native_preview() {
    let _com = prepare_windows();
    let path = std::env::temp_dir().join(format!("kiki-numbering-{}.ini", std::process::id()));
    let bounds = rect(-360, -140, 420, 400);
    let (preview, state) = capture_fixture(Image::new(bounds).unwrap(), Vec::new());
    let hwnd = preview.hwnd().unwrap();
    let (guard, warning) = preferences::initialize_at(hwnd, path.clone()).unwrap();
    assert!(warning.is_none());
    let sequence = Tool::Sequence.index().unwrap();
    let first = point(-170, -20);
    let moved = point(-100, 75);
    // SAFETY: only this test thread owns the live preview and numeric EDIT. Raw state is accessed
    // between messages, and both preference sessions are scoped to this test file.
    unsafe {
        assert!(!(*state).capture_tools[sequence]);
        assert_eq!((*state).editor.next_sequence(), 1);
        let mut cfg = preferences::current();
        cfg.capture_tools[sequence] = true;
        preferences::update(cfg).unwrap();
        SendMessageW(hwnd, preferences::CHANGED, None, None);
        let button = (*state)
            .layout()
            .unwrap()
            .buttons()
            .find(|b| b.id == tool_button(Tool::Sequence))
            .unwrap()
            .bounds;
        click(
            hwnd,
            bounds,
            point((button.left + button.right) / 2, button.top + 3),
        );
        assert_eq!((*state).editor.tool, Tool::Sequence);
        assert_eq!(
            (*state).editor.current_sequence_size(),
            DEFAULT_SEQUENCE_SIZE
        );
        click(hwnd, bounds, first);
        send_mouse(hwnd, bounds, WM_LBUTTONDOWN, first);
        send_mouse(hwnd, bounds, WM_MOUSEMOVE, moved);
        send_mouse(hwnd, bounds, WM_LBUTTONUP, moved);
        assert!(matches!((*state).editor.objects()[0].shape,
            Shape::Sequence { center, diameter: DEFAULT_SEQUENCE_SIZE } if center == moved));
        assert_eq!((*state).editor.next_sequence(), 2);

        SendMessageW(hwnd, WM_MOUSEWHEEL, Some(WPARAM(120 << 16)), None);
        SendMessageW(hwnd, WM_TIMER, Some(WPARAM(PARAMETER_TIMER)), None);
        assert_eq!((*state).editor.current_sequence_size(), 30);
        assert_eq!(preferences::current().sequence_size, 30);
        (*state).editor.undo();
        assert_eq!((*state).editor.current_sequence_size(), 28);
        assert_eq!(preferences::current().sequence_size, 30);
        (*state).editor.redo();
        for (value, expected) in [
            (w!("15"), None),
            (w!("16"), Some(16)),
            (w!("129"), None),
            (w!("128"), Some(128)),
        ] {
            open_parameter_edit(hwnd, state).unwrap();
            let edit = (*state).parameter_edit.as_ref().unwrap().hwnd;
            SetWindowTextW(edit, value).unwrap();
            SendMessageW(edit, WM_KEYDOWN, Some(WPARAM(13)), None);
            assert!((*state).parameter_edit.is_none());
            if let Some(size) = expected {
                assert!((*state).parameter_error.is_none());
                assert_eq!((*state).editor.current_sequence_size(), size);
                assert_eq!(preferences::current().sequence_size, size);
            } else {
                assert!((*state).parameter_message().unwrap().contains("16–128"));
            }
        }
        assert_eq!((*state).editor.current_sequence_size(), 128);
        let slider = (*state).layout().unwrap().parameter_slider().unwrap();
        let y = (slider.top + slider.bottom) / 2;
        click(hwnd, bounds, point(slider.left + 1, y));
        assert_eq!((*state).editor.current_sequence_size(), 16);
        click(hwnd, bounds, point(slider.right - 1, y));
        assert_eq!((*state).editor.current_sequence_size(), 128);

        let original = (*state).editor.objects().to_vec();
        preferences::set_parameter(CaptureParameter::SequenceSize, 42).unwrap();
        SendMessageW(hwnd, preferences::PARAMETER_CHANGED, None, None);
        assert_eq!((*state).editor.sequence_size, 42);
        assert_eq!((*state).editor.objects(), original);
        let mut cfg = preferences::current();
        cfg.capture_tools[sequence] = false;
        preferences::update(cfg).unwrap();
        SendMessageW(hwnd, preferences::CHANGED, None, None);
        assert_eq!((*state).editor.tool, Tool::Select);
        assert!(
            (*state)
                .layout()
                .unwrap()
                .buttons()
                .all(|b| b.id != tool_button(Tool::Sequence))
        );
        assert_eq!((*state).editor.objects(), original);
        pump_until(|| {
            preferences::poll();
            matches!(preferences::status(), preferences::SaveStatus::Saved)
        });
        drop(guard);
        let stored = Disk::load(path.clone()).config;
        assert_eq!(stored.sequence_size, 42);
        assert!(!stored.capture_tools[sequence]);

        let (guard, warning) = preferences::initialize_at(hwnd, path.clone()).unwrap();
        assert!(warning.is_none());
        let (next, next_state) = capture_fixture(Image::new(bounds).unwrap(), Vec::new());
        assert_eq!((*next_state).editor.next_sequence(), 1);
        assert_eq!((*next_state).editor.current_sequence_size(), 42);
        drop(next);
        preferences::reset_capture_defaults().unwrap();
        SendMessageW(hwnd, preferences::CHANGED, None, None);
        assert_eq!(preferences::current().sequence_size, 28);
        assert!(!preferences::current().capture_tools[sequence]);
        assert_eq!((*state).editor.objects(), original);
        pump_until(|| {
            preferences::poll();
            matches!(preferences::status(), preferences::SaveStatus::Saved)
        });
        drop(guard);
    }
    assert!(take_ui_errors().is_empty());
    std::fs::remove_file(path).unwrap();
}

fn picker_fixture() -> (Preview, *mut State) {
    let bounds = rect(-360, -140, 420, 400);
    let mut image = Image::new(bounds).unwrap();
    let stride = image.stride() as usize;
    for (i, pixel) in image.pixels_mut().chunks_exact_mut(4).enumerate() {
        let x = bounds.left + (i % (stride / 4)) as i32;
        let bgra = match x {
            ..=-161 => [86, 95, 54, 255],
            -160..=-11 => [187, 206, 160, 255],
            -10..=9 => [0, 0, 0, 0],
            _ => [74, 167, 13, 255],
        };
        pixel.copy_from_slice(&bgra);
    }
    let screens = vec![
        DisplayInfo {
            bounds: rect(-360, -140, -10, 400),
            name: "fixture-left".into(),
            scale_percent: 100,
            rotation_degrees: 0,
            refresh_hz: 60,
            color: ColorMode::Sdr,
        },
        DisplayInfo {
            bounds: rect(10, -140, 420, 400),
            name: "fixture-right".into(),
            scale_percent: 200,
            rotation_degrees: 0,
            refresh_hz: 60,
            color: ColorMode::Sdr,
        },
    ];
    let (preview, state) = capture_fixture(image, screens);
    // SAFETY: only the test owns this window; the region and tool list are set before dispatch.
    unsafe {
        (*state).selection.rect = Some(rect(-330, -90, 390, 340));
        (*state).capture_tools[Tool::Picker.index().unwrap()] = true;
    }
    (preview, state)
}

#[test]
fn picker_routes_native_shift_and_samples_frozen_physical_pixels() {
    let _com = prepare_windows();
    let (preview, state) = picker_fixture();
    let hwnd = preview.hwnd().unwrap();
    // SAFETY: all input and state inspection target only this thread's hidden test window.
    unsafe {
        let bounds = (*state).canvas.bounds();
        let encoded = |state: *mut State| {
            (*state)
                .picker_readout()
                .map(|r| r.rgb.map(|rgb| r.format.encode(rgb)))
        };
        let shift = |msg, flags| {
            SendMessageW(
                hwnd,
                msg,
                Some(WPARAM(usize::from(VK_SHIFT.0))),
                Some(LPARAM(flags)),
            )
        };
        (*state).editor.tool = Tool::Rectangle;
        (*state).editor.begin_at(point(-160, -60), None).unwrap();
        (*state).editor.finish(point(-115, 60));
        (*state).editor.tool = Tool::Mosaic;
        (*state).editor.begin_at(point(-174, 16), None).unwrap();
        (*state).editor.finish(point(-135, 66));
        let original = (*state).editor.objects().to_vec();
        let composed = composition::render(
            &(*state).canvas.source(),
            (*state).selection.rect.unwrap(),
            &(*state).editor,
            0,
        )
        .unwrap();
        assert_ne!(composed.rgb_at(point(-160, 34)), Some([160, 206, 187]));

        (*state).activate(tool_button(Tool::Picker));
        for (p, expected) in [
            (point(-161, 34), Some(Some("#365F56"))),
            (point(-160, 34), Some(Some("#A0CEBB"))),
            (point(-1, 32), Some(None)),
            (point(20, 32), Some(Some("#0DA74A"))),
            (point(-335, 32), None),
        ] {
            send_mouse(hwnd, bounds, WM_MOUSEMOVE, p);
            assert_eq!(encoded(state).as_ref().map(|v| v.as_deref()), expected);
        }
        (*state).selection.rect = Some(bounds);
        let toolbar = (*state).layout().unwrap().toolbar;
        assert!(contains(
            (*state).selection.rect.unwrap(),
            point(toolbar.left + 5, toolbar.top + 5)
        ));
        send_mouse(
            hwnd,
            bounds,
            WM_MOUSEMOVE,
            point(toolbar.left + 5, toolbar.top + 5),
        );
        assert!(encoded(state).is_none());

        send_mouse(hwnd, bounds, WM_MOUSEMOVE, point(-160, 34));
        shift(WM_KEYDOWN, 1);
        shift(WM_KEYDOWN, 1 | (1 << 30));
        shift(WM_KEYDOWN, 1 | (1 << 30));
        shift(WM_KEYUP, 1 | (3 << 30));
        assert_eq!(
            encoded(state).flatten().as_deref(),
            Some("RGB(160, 206, 187)")
        );
        shift(WM_KEYDOWN, 1);
        assert_eq!(encoded(state).flatten().as_deref(), Some("#A0CEBB"));
        shift(WM_KEYDOWN, 1);
        assert_eq!(
            encoded(state).flatten().as_deref(),
            Some("RGB(160, 206, 187)")
        );
        (*state).activate(tool_button(Tool::Text));
        start_text(hwnd, state, None).unwrap();
        shift(WM_KEYDOWN, 1);
        shift(WM_KEYUP, 1 | (3 << 30));
        SendMessageW(hwnd, WM_CHAR, Some(WPARAM('A' as usize)), None);
        assert_eq!(
            (*state).text_input.as_ref().unwrap().content().unwrap(),
            "A"
        );
        SendMessageW(hwnd, WM_KEYDOWN, Some(WPARAM(27)), None);
        assert!((*state).text_input.is_none());
        (*state).activate(tool_button(Tool::Picker));
        send_mouse(hwnd, bounds, WM_MOUSEMOVE, point(-160, 34));
        assert_eq!(encoded(state).flatten().as_deref(), Some("#A0CEBB"));
        assert_eq!((*state).editor.objects(), original);
        SendMessageW(hwnd, preferences::CHANGED, None, None);
        assert_eq!((*state).editor.tool, Tool::Select);
        assert!(!(*state).activate(tool_button(Tool::Picker)));
        assert!(encoded(state).is_none());
        assert_eq!((*state).editor.objects(), original);
    }
    assert!(take_ui_errors().is_empty());
}

#[test]
fn numbering_history_restores_order_and_renumbers_the_rendered_digits() {
    let bounds = rect(-40, -30, 560, 64);
    let mut source = Image::new(bounds).unwrap();
    source.pixels_mut().fill(255);
    let mut editor = Editor::default();
    editor.tool = Tool::Sequence;
    let positions: Vec<_> = (0..12).map(|i| point(-8 + 46 * i, 12)).collect();
    for p in &positions {
        editor.begin_at(*p, None).unwrap();
        editor.finish(*p);
    }
    assert_eq!(editor.next_sequence(), 13);
    let original = editor.objects().to_vec();
    let render = |e: &Editor| composition::render(&source, bounds, e, 0).unwrap();
    let original_image = render(&editor);
    let patch = |image: &Image, center: PixelPoint| {
        image
            .region(rect(center.x - 9, center.y - 9, center.x + 9, center.y + 9))
            .unwrap()
            .pixels()
            .to_vec()
    };
    assert!(patch(&original_image, positions[8]) != patch(&original_image, positions[9]));
    for index in [4, 0, 11] {
        editor.selected = Some(index);
        editor.delete();
        let mut remaining = original.clone();
        remaining.remove(index);
        assert_eq!(editor.objects(), remaining);
        assert_eq!(editor.next_sequence(), 12);
        let shortened = render(&editor);
        if index + 1 < positions.len() {
            assert!(
                patch(&shortened, positions[index + 1]) == patch(&original_image, positions[index]),
                "the digit after deletion at index {index} did not match its new ordinal"
            );
        }
        editor.undo();
        assert_eq!(editor.objects(), original);
        assert_eq!(render(&editor).pixels(), original_image.pixels());
        editor.redo();
        assert_eq!(editor.objects(), remaining);
        assert_eq!(render(&editor).pixels(), shortened.pixels());
        editor.undo();
    }
    editor.selected = Some(4);
    editor.delete();
    let next = point(545, 40);
    editor.begin_at(next, None).unwrap();
    editor.finish(next);
    assert_eq!(editor.next_sequence(), 13);
    assert!(patch(&render(&editor), next) == patch(&original_image, positions[11]));
    assert!(!editor.can_redo());
}

fn numbered_output_fixture() -> (Preview, *mut State, Image) {
    let bounds = rect(-110, -60, 260, 260);
    let area = rect(-80, -30, 220, 220);
    let mut image = Image::new(bounds).unwrap();
    image.pixels_mut().fill(255);
    let (preview, state) = capture_fixture(image, Vec::new());
    // SAFETY: fixture state is exclusively owned by this test's hidden preview.
    unsafe {
        (*state).selection.rect = Some(area);
        (*state).corner_radius = 20;
        let editor = &mut (*state).editor;
        editor.tool = Tool::Sequence;
        for (center, diameter) in [(point(-75, 50), 48), (point(90, 90), 128)] {
            editor.sequence_size = diameter;
            editor.begin_at(center, None).unwrap();
            editor.finish(center);
        }
        let expected = composition::render(&(*state).canvas.source(), area, editor, 20).unwrap();
        assert_eq!(expected.rgb_at(point(-80, -30)), None);
        assert_eq!(expected.rgb_at(point(-80, 27)), Some([255, 59, 64]));
        assert_eq!(expected.rgb_at(point(90, 27)), Some([255, 59, 64]));
        (preview, state, expected)
    }
}

#[test]
fn numbering_exports_png_jpeg_and_pins_the_same_composed_image() {
    let _com = prepare_windows();
    let dir = std::env::temp_dir().join(format!("kiki-numbered-output-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for format in [OutputFormat::Png, OutputFormat::Jpeg] {
        let (preview, state, expected) = numbered_output_fixture();
        let path = dir.join(format!("sequence.{}", format.extension()));
        let target = output::SaveTarget::new(path.clone(), format).unwrap();
        output::TEST_DIALOG.with_borrow_mut(|path| *path = Some(Some(target)));
        // SAFETY: the save dialog override and output command belong to this test thread.
        unsafe {
            (*state).pending_output = Some(8);
            begin_output(preview.hwnd().unwrap(), state).unwrap();
        }
        pump_until(|| !preview.is_open());
        let (size, pixels) = super::super::image_encode::decode_file(&path);
        assert_eq!(size, expected.bounds().size().unwrap());
        if format == OutputFormat::Png {
            assert_eq!(pixels, expected.pixels());
        } else {
            let at = |x: usize, y: usize| {
                let i = y * expected.stride() as usize + x * 4;
                &pixels[i..i + 4]
            };
            assert!(at(0, 0).iter().all(|&value| value >= 250));
            assert!(at(170, 57)[2] > 200 && at(170, 57)[1] < 100);
        }
        std::fs::remove_file(path).unwrap();
    }
    let (preview, state, expected) = numbered_output_fixture();
    // SAFETY: the pin command is posted to the owning thread's live preview.
    unsafe {
        (*state).pending_output = Some(7);
        begin_output(preview.hwnd().unwrap(), state).unwrap();
    }
    pump_until(|| !preview.is_open());
    let actual = super::super::pinned::test_image().unwrap();
    assert_eq!(actual.bounds(), expected.bounds());
    assert_eq!(actual.pixels(), expected.pixels());
    super::super::pinned::close_all();
    assert!(take_ui_errors().is_empty());
    std::fs::remove_dir(dir).unwrap();
}

struct OpenTestClipboard;
impl OpenTestClipboard {
    fn open(owner: HWND) -> Self {
        // SAFETY: the owning test thread closes the open clipboard in Drop.
        unsafe { OpenClipboard(Some(owner)).unwrap() };
        Self
    }

    fn data(&self, format: u32) -> Vec<u8> {
        // SAFETY: the clipboard remains open until after the call, and the locked storage
        // is read only within its allocation size and unlocked before returning.
        unsafe {
            let handle = HGLOBAL(GetClipboardData(format).unwrap().0);
            let size = GlobalSize(handle);
            let pointer = GlobalLock(handle).cast::<u8>();
            assert!(!pointer.is_null());
            let bytes = std::slice::from_raw_parts(pointer, size).to_vec();
            let _ = GlobalUnlock(handle);
            bytes
        }
    }
}
impl Drop for OpenTestClipboard {
    fn drop(&mut self) {
        // SAFETY: balances the same test thread's OpenClipboard.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn require_empty_clipboard(owner: HWND) {
    let _clipboard = OpenTestClipboard::open(owner);
    // SAFETY: only query the number of formats; do not read or replace existing contents.
    unsafe {
        assert_eq!(
            CountClipboardFormats(),
            0,
            "desktop clipboard must be empty"
        )
    };
}

struct ClearTestClipboard(HWND);
impl Drop for ClearTestClipboard {
    fn drop(&mut self) {
        // SAFETY: clear only data still owned by the test window, while holding the clipboard open.
        unsafe {
            if OpenClipboard(Some(self.0)).is_ok() {
                let _clipboard = OpenTestClipboard;
                if GetClipboardOwner().ok() == Some(self.0) {
                    let _ = EmptyClipboard();
                }
            }
        }
    }
}

#[test]
#[ignore = "writes to a verified empty desktop clipboard; run serially"]
fn clipboard_roundtrips_picker_text_and_numbered_image() {
    let _com = prepare_windows();
    let (preview, state) = picker_fixture();
    let hwnd = preview.hwnd().unwrap();
    require_empty_clipboard(hwnd);
    let cleanup = ClearTestClipboard(hwnd);
    // SAFETY: this test owns the preview and sends only test messages to it.
    unsafe {
        let bounds = (*state).canvas.bounds();
        let color = point(-100, 50);
        (*state).activate(tool_button(Tool::Picker));
        let (locker, _) = capture_fixture(Image::new(rect(0, 0, 20, 20)).unwrap(), Vec::new());
        {
            let _lock = OpenTestClipboard::open(locker.hwnd().unwrap());
            send_mouse(hwnd, bounds, WM_LBUTTONDOWN, color);
            assert_eq!((*state).picker_down, Some(color));
            send_mouse(hwnd, bounds, WM_LBUTTONUP, color);
            assert!((*state).output.is_some(), "no color-copy job was started");
            pump_until(|| (*state).output.is_none());
            assert_eq!(
                (*state).picker_notice.map(PickerNotice::label),
                Some(PickerNotice::Failed.label())
            );
            assert!(preview.is_open());
        }
        drop(locker);
        for (location, expected) in [(color, "#A0CEBB"), (point(40, 60), "RGB(13, 167, 74)")] {
            if location != color {
                SendMessageW(
                    hwnd,
                    WM_KEYDOWN,
                    Some(WPARAM(usize::from(VK_SHIFT.0))),
                    Some(LPARAM(1)),
                );
            }
            click(hwnd, bounds, location);
            pump_until(|| (*state).output.is_none());
            assert!((*state).picker_notice == Some(PickerNotice::Copied));
            assert!(preview.is_open());
            let clipboard = OpenTestClipboard::open(hwnd);
            let data = clipboard.data(u32::from(CF_UNICODETEXT.0));
            let text: Vec<_> = data
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&c| c != 0)
                .collect();
            assert_eq!(String::from_utf16(&text).unwrap(), expected);
        }
        drop(cleanup);
        let (numbered, numbered_state, expected) = numbered_output_fixture();
        let owner = numbered.hwnd().unwrap();
        require_empty_clipboard(owner);
        let _cleanup = ClearTestClipboard(owner);
        let job = output::start(
            (*numbered_state).canvas.source(),
            (*numbered_state).selection.rect.unwrap(),
            (*numbered_state).editor.snapshot(),
            (*numbered_state).corner_radius,
            owner.0 as usize,
            output::Destination::Clipboard,
        )
        .unwrap();
        pump_until(|| match job.receiver.try_recv() {
            Ok(result) => {
                assert!(matches!(result.unwrap(), output::Completed::Done));
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(error) => panic!("{error}"),
        });
        let clipboard = OpenTestClipboard::open(owner);
        let png = clipboard.data(RegisterClipboardFormatW(w!("PNG")));
        let path = std::env::temp_dir().join(format!(
            "kiki-numbered-clipboard-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, png).unwrap();
        let (size, pixels) = super::super::image_encode::decode_file(&path);
        std::fs::remove_file(path).unwrap();
        assert_eq!(size, expected.bounds().size().unwrap());
        assert_eq!(pixels, expected.pixels());
    }
    assert!(take_ui_errors().is_empty());
}

fn long_edit_fixture(scale_percent: i32) -> (Preview, *mut State, PixelPoint) {
    use crate::core::long_image::{fixture, thumbnail};
    let desktop = rect(0, 0, 900, 660);
    let screen = DisplayInfo {
        bounds: desktop,
        name: "long-document-fixture".into(),
        scale_percent,
        rotation_degrees: 0,
        refresh_hz: 60,
        color: ColorMode::Sdr,
    };
    let (preview, state) = capture_fixture(Image::new(desktop).unwrap(), vec![screen.clone()]);
    let mut image = fixture(1000, 0, 4350);
    let bounds = image.bounds();
    let stride = image.stride() as usize;
    let sample = point(440, 4280);
    for y in 4260..4300 {
        for pixel in
            image.pixels_mut()[y * stride + 420 * 4..y * stride + 460 * 4].chunks_exact_mut(4)
        {
            pixel.copy_from_slice(&[229, 243, 222, 255]);
        }
    }
    let thumbnail = thumbnail(&image).unwrap();
    // SAFETY: the test exclusively owns the hidden Preview and installs a finished
    // document before dispatching any input to its HWND.
    unsafe {
        (*state).long_document = LongDocument::new(&screen, &image, thumbnail);
        assert!((*state).long_document.is_some());
        (*state).canvas.set_document(image);
        (*state).selection = Selection::new(bounds);
        (*state).selection.rect = Some(bounds);
        (*state).corner_radius = 24;
        (*state).capture_tools[Tool::Picker.index().unwrap()] = true;
        (*state).editor.tool = Tool::Rectangle;
        for y in [60, 2100, 4290] {
            (*state).editor.begin_at(point(60, y), None).unwrap();
            (*state).editor.finish(point(170, y + 30));
        }
    }
    (preview, state, sample)
}

#[test]
fn undo_button_is_visible_and_uses_history_in_both_editors() {
    let _com = prepare_windows();
    for long in [false, true] {
        let (preview, state) = if long {
            let (preview, state, _) = long_edit_fixture(150);
            (preview, state)
        } else {
            capture_fixture(Image::new(rect(0, 0, 900, 660)).unwrap(), Vec::new())
        };
        let hwnd = preview.hwnd().unwrap();
        // SAFETY: the test owns this hidden preview and sends messages only to its HWND.
        unsafe {
            (*state).editor.clear_document();
            (*state).editor.tool = Tool::Select;
            let desktop = (*state).canvas.window_bounds();
            let layout = (*state).layout().unwrap();
            let undo = layout
                .buttons()
                .find(|b| b.id == capture_toolbar::UNDO)
                .unwrap()
                .bounds;
            let undo_center = point((undo.left + undo.right) / 2, (undo.top + undo.bottom) / 2);
            assert!(!(*state).can_focus(&layout, capture_toolbar::UNDO));
            click(hwnd, desktop, undo_center);
            assert!((*state).editor.objects().is_empty());

            (*state).editor.tool = Tool::Rectangle;
            (*state).editor.begin_at(point(60, 60), None).unwrap();
            (*state).editor.finish(point(170, 100));
            (*state).editor.tool = Tool::Select;
            assert!((*state).can_focus(&layout, capture_toolbar::UNDO));
            click(hwnd, desktop, undo_center);
            assert!((*state).editor.objects().is_empty());
            assert!(!(*state).can_focus(&layout, capture_toolbar::UNDO));

            (*state).editor.redo();
            assert_eq!((*state).editor.objects().len(), 1);
            (*state).keyboard_focus = true;
            (*state).focus = capture_toolbar::UNDO;
            SendMessageW(hwnd, WM_KEYDOWN, Some(WPARAM(13)), None);
            assert!((*state).editor.objects().is_empty());
            assert!((*state).editor.can_redo());
            assert!(preview.is_open());
        }
    }
    assert!(take_ui_errors().is_empty());
}

fn navigate_long_to_bottom(preview: &Preview, state: *mut State) {
    // SAFETY: these coordinates and the live state belong to the test-owned Preview.
    unsafe {
        let track = (*state).long_document.as_ref().unwrap().scrollbar();
        click(
            preview.hwnd().unwrap(),
            (*state).canvas.window_bounds(),
            point(track.left + 1, track.bottom - 1),
        );
        let view = (*state).long_document.as_ref().unwrap().view;
        assert!(view.scale < 1.);
        assert_eq!(view.offset, view.max_offset());
        assert_eq!(view.band().bottom, 4350);
    }
}

#[test]
fn long_document_navigation_and_rgb_use_physical_document_pixels_at_two_scales() {
    let _com = prepare_windows();
    for scale_percent in [100, 150] {
        let (preview, state, sample) = long_edit_fixture(scale_percent);
        let hwnd = preview.hwnd().unwrap();
        navigate_long_to_bottom(&preview, state);
        // SAFETY: the test owns this hidden Preview and dispatches only to its own HWND.
        unsafe {
            let view = (*state).long_document.as_ref().unwrap().view;
            let location = view.to_screen(sample);
            let bounds = (*state).canvas.window_bounds();
            (*state).activate(tool_button(Tool::Picker));
            send_mouse(hwnd, bounds, WM_MOUSEMOVE, location);
            let readout = || {
                let readout = (*state).picker_readout().unwrap();
                readout.format.encode(readout.rgb.unwrap())
            };
            assert_eq!(readout(), "#DEF3E5");
            SendMessageW(
                hwnd,
                WM_KEYDOWN,
                Some(WPARAM(usize::from(VK_SHIFT.0))),
                Some(LPARAM(1)),
            );
            SendMessageW(
                hwnd,
                WM_KEYUP,
                Some(WPARAM(usize::from(VK_SHIFT.0))),
                Some(LPARAM(1 | (3 << 30))),
            );
            assert_eq!(readout(), "RGB(222, 243, 229)");
            SendMessageW(
                hwnd,
                WM_KEYDOWN,
                Some(WPARAM(usize::from(VK_SHIFT.0))),
                Some(LPARAM(1)),
            );
            assert_eq!(readout(), "#DEF3E5");
        }
    }
    assert!(take_ui_errors().is_empty());
}

#[test]
#[ignore = "writes RGB text to a verified empty desktop clipboard; run serially"]
fn long_document_rgb_copies_exact_text_and_keeps_editing() {
    let _com = prepare_windows();
    let (preview, state, sample) = long_edit_fixture(150);
    let hwnd = preview.hwnd().unwrap();
    require_empty_clipboard(hwnd);
    let _cleanup = ClearTestClipboard(hwnd);
    navigate_long_to_bottom(&preview, state);
    // SAFETY: the test owns the Preview, clipboard guard and all synthetic input.
    unsafe {
        let location = (*state)
            .long_document
            .as_ref()
            .unwrap()
            .view
            .to_screen(sample);
        let bounds = (*state).canvas.window_bounds();
        (*state).activate(tool_button(Tool::Picker));
        send_mouse(hwnd, bounds, WM_MOUSEMOVE, location);
        SendMessageW(
            hwnd,
            WM_KEYDOWN,
            Some(WPARAM(usize::from(VK_SHIFT.0))),
            Some(LPARAM(1)),
        );
        click(hwnd, bounds, location);
        pump_until(|| (*state).output.is_none());
        assert!(preview.is_open());
        assert!((*state).picker_notice == Some(PickerNotice::Copied));
        let clipboard = OpenTestClipboard::open(hwnd);
        let units: Vec<_> = clipboard
            .data(u32::from(CF_UNICODETEXT.0))
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|&unit| unit != 0)
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "RGB(222, 243, 229)");
    }
    assert!(take_ui_errors().is_empty());
}

#[test]
fn long_document_save_cancel_locked_target_and_retry_preserve_full_output() {
    use std::os::windows::fs::OpenOptionsExt;
    let _com = prepare_windows();
    let (preview, state, _) = long_edit_fixture(150);
    let hwnd = preview.hwnd().unwrap();
    navigate_long_to_bottom(&preview, state);
    let dir = std::env::temp_dir().join(format!("kiki-f11-save-retry-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("long.png");
    std::fs::write(&path, b"original file").unwrap();
    let target = output::SaveTarget::new(path.clone(), OutputFormat::Png).unwrap();
    let save = |destination| {
        output::TEST_DIALOG.with_borrow_mut(|next| *next = Some(destination));
        // SAFETY: the test owns the hidden Preview, which only frees State on successful output.
        unsafe {
            (*state).pending_output = Some(8);
            begin_output(hwnd, state).unwrap();
            pump_until(|| !preview.is_open() || !(*state).busy);
        }
    };
    // SAFETY: snapshot these values while the test-owned Preview is still live.
    let (offset, annotations, expected) = unsafe {
        let source = (*state).canvas.source();
        (
            (*state).long_document.as_ref().unwrap().view.offset,
            (*state).editor.objects().to_vec(),
            composition::render(
                &source,
                source.bounds(),
                &(*state).editor,
                (*state).corner_radius,
            )
            .unwrap(),
        )
    };
    let assert_retained = || {
        assert!(preview.is_open());
        // SAFETY: the liveness check above confirms this test still owns the same State.
        unsafe {
            assert_eq!((*state).long_document.as_ref().unwrap().view.offset, offset);
            assert_eq!((*state).editor.objects(), annotations);
        }
    };
    save(None);
    assert_retained();
    assert!(take_ui_errors().is_empty());
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    save(Some(target.clone()));
    assert_retained();
    assert_eq!(take_ui_errors().len(), 1);
    drop(locked);
    assert_eq!(std::fs::read(&path).unwrap(), b"original file");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    save(Some(target));
    assert!(!preview.is_open());
    assert!(take_ui_errors().is_empty());
    let (size, pixels) = super::super::image_encode::decode_file(&path);
    assert_eq!(size, (1000, 4350));
    assert_eq!(pixels, expected.pixels());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn cross_screen_long_capture_requests_reselection_and_next_preview_is_ordinary() {
    let _com = prepare_windows();
    let (preview, state) = picker_fixture();
    let hwnd = preview.hwnd().unwrap();
    // SAFETY: both fixture screens and the pending long-capture action belong to the test.
    unsafe {
        (*state).start_long = true;
        assert!(start_long_if_requested(hwnd, state));
        assert!((*state).long_reselect.is_some());
        assert!((*state).selection.rect.is_none());
        assert!((*state).long_capture.is_none());
        assert!((*state).long_document.is_none());
        let bounds = (*state).canvas.window_bounds();
        (*state).windows = vec![bounds];
        click(hwnd, bounds, point(bounds.left + 10, bounds.top + 10));
        assert!((*state).long_reselect.is_some());
        assert!(
            (*state).selection.rect.is_none(),
            "a click selected a full window during reselection"
        );
        assert!((*state).long_capture.is_none());
        SendMessageW(hwnd, WM_KEYDOWN, Some(WPARAM(VK_ESCAPE.0 as usize)), None);
        assert!(!preview.is_open());
    }
    drop(preview);
    let (_next, next_state) = picker_fixture();
    // SAFETY: the fresh ordinary Preview is still live and belongs to this thread.
    unsafe {
        assert!((*next_state).long_reselect.is_none());
        assert!((*next_state).long_document.is_none());
        assert!((*next_state).long_capture.is_none());
        assert!((*next_state).selection.rect.is_some());
    }
    assert!(take_ui_errors().is_empty());
}

#[test]
#[ignore = "uses a visible native source and the real Windows preview"]
fn long_capture_toolbar_preserves_viewport_and_leaves_a_recoverable_session() {
    use long_capture::tests::{native_document, route_test_wheel};

    let _com = prepare_windows();
    let screen = super::super::display::enumerate()
        .unwrap()
        .into_iter()
        .map(|screen| screen.info)
        .find(|screen| {
            screen.color == ColorMode::Sdr
                && screen
                    .bounds
                    .size()
                    .is_some_and(|(w, h)| w >= 800 && h >= 600)
        })
        .expect("requires a real SDR display");
    println!("display={screen:?}");
    let (source, _, area, _) = native_document(&screen);
    #[cfg(feature = "diagnostics")]
    let desktop_capture = || {
        std::thread::spawn(super::super::capture::desktop)
            .join()
            .unwrap()
            .unwrap()
    };
    #[cfg(feature = "diagnostics")]
    let source_pixels = desktop_capture().image.region(area).unwrap();
    let bounds = screen.bounds;
    let (preview, state) = capture_fixture(Image::new(bounds).unwrap(), vec![screen]);
    let hwnd = preview.hwnd().unwrap();
    let center = POINT {
        x: area.left + 250,
        y: area.top + 100,
    };
    // SAFETY: the test owns these native windows and dispatches messages on their UI thread.
    unsafe {
        (*state).capture_tools[Tool::LongCapture.index().unwrap()] = true;
        (*state).selection.rect = Some(area);
        let button = (*state)
            .layout()
            .unwrap()
            .buttons()
            .find(|button| button.id == capture_toolbar::LONG_CAPTURE)
            .unwrap()
            .bounds;
        let _ = ShowWindow(hwnd, SW_SHOW);
        click(
            hwnd,
            bounds,
            point((button.left + button.right) / 2, button.top + 5),
        );
        assert_eq!(
            (*state).long_capture.as_ref().map(|capture| capture.area),
            Some(area)
        );
        assert_eq!((*state).selection.rect, Some(area));
        assert_eq!(GetAncestor(WindowFromPoint(center), GA_ROOT), source.0);
        let capture = (*state).long_capture.as_ref().unwrap();
        assert!(capture.panel.bounds.intersection(area).is_none());
        let mut affinity = u32::MAX;
        GetWindowDisplayAffinity(hwnd, &mut affinity).unwrap();
        assert_eq!(affinity, WDA_NONE.0);

        pump_until(|| {
            (*state).long_reselect.is_some()
                || (*state)
                    .long_capture
                    .as_ref()
                    .is_some_and(|capture| capture.update.is_some())
        });
        if let Some(message) = (*state).long_reselect {
            assert!(message.contains("无法获取长截图首帧"));
            assert!((*state).selection.rect.is_none());
            assert!((*state).long_capture.is_none());
            assert_eq!(WindowFromPoint(center), hwnd);
            println!("initial capture unavailable, drawing and cancellation restored");
            SendMessageW(hwnd, WM_KEYDOWN, Some(WPARAM(VK_ESCAPE.0 as usize)), None);
        } else {
            let capture = (*state).long_capture.as_ref().unwrap();
            let (width, height) = area.size().unwrap();
            assert_eq!(capture.update.as_ref().unwrap().height, height);
            let finish = capture.panel.finish;
            #[cfg(feature = "diagnostics")]
            {
                // Capture the desktop through WGC, just as a screen-sharing client
                // would. The finish button must be visible in that capture.
                let _ = UpdateWindow(hwnd);
                windows::Win32::Graphics::Dwm::DwmFlush().unwrap();
                let desktop = desktop_capture();
                let accent = super::super::ui_palette::accent((*state).canvas.dark);
                let expected = [(accent >> 16) as u8, (accent >> 8) as u8, accent as u8];
                let sample = point(
                    finish.left + (finish.right - finish.left) / 4,
                    (finish.top + finish.bottom) / 2,
                );
                assert_eq!(desktop.image.rgb_at(sample), Some(expected));
                assert_eq!(
                    desktop.image.region(area).unwrap().pixels(),
                    source_pixels.pixels()
                );
                println!(
                    "desktop capture includes the control panel and preserves the source pixels"
                );
            }
            let start = Instant::now();
            route_test_wheel(point(center.x, center.y), -120);
            pump_until(|| {
                (*state)
                    .long_capture
                    .as_ref()
                    .unwrap()
                    .update
                    .as_ref()
                    .unwrap()
                    .height
                    > height
            });
            let grown = (*state)
                .long_capture
                .as_ref()
                .unwrap()
                .update
                .as_ref()
                .unwrap()
                .height;
            println!(
                "preview_height={height}->{grown}, wheel_to_preview_ms={:.1}",
                start.elapsed().as_secs_f64() * 1000.
            );
            click(hwnd, bounds, point(finish.left + 2, finish.top + 2));
            pump_until(|| (*state).long_document.is_some());
            let image = (*state).canvas.source();
            #[cfg(feature = "diagnostics")]
            assert_eq!(
                &image.pixels()[..source_pixels.pixels().len()],
                source_pixels.pixels(),
                "the first captured viewport must exclude the previous screenshot UI"
            );
            let path =
                std::env::temp_dir().join(format!("kiki-long-toolbar-{}.png", std::process::id()));
            let target = output::SaveTarget::new(path.clone(), OutputFormat::Png).unwrap();
            output::TEST_DIALOG.with_borrow_mut(|next| *next = Some(Some(target)));
            (*state).corner_radius = 0;
            (*state).pending_output = Some(8);
            begin_output(hwnd, state).unwrap();
            pump_until(|| !preview.is_open());
            let (size, pixels) = super::super::image_encode::decode_file(&path);
            std::fs::remove_file(path).unwrap();
            assert_eq!(size, (width, grown));
            assert_eq!(pixels, image.pixels());
            println!("saved_png={width}x{grown}, decoded pixels match the stitched image");
        }
        assert!(!preview.is_open());
    }
    assert!(take_ui_errors().is_empty());
}
